use std::collections::HashMap;
use std::{collections::HashSet, sync::Arc};

use crate::agent::{Agent, AgentStatus};
use crate::payload::{construct_payload, Payload};
use crate::request::{Identifier, Request, construct_request};
use crate::response::{Response, Status};
use crate::{ecdh_decrypt, ecdh_encrypt, MAX_TRANSMIT_SIZE};
use futures::channel::oneshot;
use futures::{SinkExt, StreamExt};
use rust_ipfs::libp2p::gossipsub::Message;
use rust_ipfs::{Ipfs, Keypair, PeerId, PublicKey};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug)]
pub enum ClientCommand<'a> {
    Store {
        namespace: Vec<u8>,
        payload: Payload<'a>,
        response: oneshot::Sender<anyhow::Result<ResponseType>>,
    },
    Find {
        namespace: Vec<u8>,
        response: oneshot::Sender<anyhow::Result<ResponseType>>,
    },
    Delete {
        namespace: Vec<u8>,
        response: oneshot::Sender<anyhow::Result<Status>>,
    },
}

pub enum ResponseType {
    ResponseOwned(Option<PeerId>, Vec<u8>),
    Status(Status),
    StatusWithData(Status, Vec<u8>),
}

impl From<Status> for ResponseType {
    fn from(value: Status) -> Self {
        ResponseType::Status(value)
    }
}

#[allow(dead_code)]

pub struct ShuttleClient {
    // Ipfs instance
    ipfs: Ipfs,

    // List of agents
    agents: Arc<RwLock<HashSet<Agent>>>,

    // client task
    task: Arc<tokio::task::JoinHandle<anyhow::Result<()>>>,

    // Sender
    tx: futures::channel::mpsc::Sender<ClientCommand<'static>>,
}

impl ShuttleClient {
    pub fn new(ipfs: Ipfs) -> Self {
        let (tx, rx) = futures::channel::mpsc::channel(1);
        let agents: Arc<RwLock<HashSet<Agent>>> = Default::default();
        let task = Arc::new(tokio::spawn({
            let ipfs = ipfs.clone();
            let agents = agents.clone();
            let mut rx = rx;
            async move {
                let keypair = ipfs.keypair()?;
                let peer_id = keypair.public().to_peer_id();

                let response = ipfs
                    .pubsub_subscribe(format!("/shuttle/response/{peer_id}"))
                    .await?;

                let mut awaiting_response: HashMap<
                    Uuid,
                    futures::channel::oneshot::Sender<anyhow::Result<ResponseType>>,
                > = HashMap::new();
                futures::pin_mut!(response);

                loop {
                    tokio::select! {
                        biased;
                        Some(command) = rx.next() => {
                            if let Err(_e) = process_command(&ipfs, keypair, agents.clone(), &mut awaiting_response, command).await {}
                        },
                        Some(response) = response.next() => {
                            if let Err(_e) = process_message(keypair, response, agents.clone(), &mut awaiting_response).await {}
                        }
                    }
                }
            }
        }));
        Self {
            ipfs,
            agents,
            task,
            tx,
        }
    }

    pub async fn add_agent<A: Into<Agent>>(
        &mut self,
        agent: A,
        offline: bool,
    ) -> Result<(), anyhow::Error> {
        let mut agent = agent.into();
        if !offline {
            agent.connect().await?;
        }
        self.agents.write().await.insert(agent);

        Ok(())
    }

    pub async fn remove_agent(&mut self, peer_id: PeerId) -> Result<(), anyhow::Error> {
        let agent = self
            .agents
            .read()
            .await
            .iter()
            .find(|agent| agent.peer_id() == peer_id)
            .cloned();
        if let Some(agent) = &agent {
            self.agents.write().await.remove(agent);
        }
        Ok(())
    }
}

impl ShuttleClient {
    pub async fn store<S: Serialize, N: AsRef<[u8]>, P: Into<PublicKey>>(
        &self,
        recipient: P,
        namespace: N,
        data: S,
    ) -> Result<(), anyhow::Error> {
        anyhow::ensure!(self.agents.read().await.len() > 0);
        let keypair = self.ipfs.keypair()?;

        let recipient = recipient.into();
        let namespace = namespace.as_ref();
        let data = bincode::serialize(&data)?;
        let payload = construct_payload(keypair, &recipient, &data)?;
        let (tx, rx) = oneshot::channel();

        let command = ClientCommand::Store {
            namespace: namespace.into(),
            payload,
            response: tx,
        };

        self.tx.clone().send(command).await?;
        

        let result = rx.await??;

        match result {
            ResponseType::ResponseOwned(_, _) => {},
            ResponseType::Status(_) => {},
            ResponseType::StatusWithData(_, _) => {}
        }
        Ok(())
    }

    pub async fn find<D: DeserializeOwned, N: AsRef<[u8]>, P: Into<PublicKey>>(
        &self,
        _agent: Option<PeerId>,
        _recipient: P,
        _namespace: N,
    ) -> Result<D, anyhow::Error> {
        anyhow::bail!("unimplemented")
    }
}

impl ShuttleClient {
    pub async fn connected_agents(&self) -> Result<Vec<PeerId>, anyhow::Error> {
        let mut agents = vec![];
        let current_agents = self.agents.read().await;
        for agent in &*current_agents {
            //Since status is online, check to make sure the agents is subscribed to the general topic
            let list = self
                .ipfs
                .pubsub_peers(Some("/shuttle/announce".into()))
                .await
                .unwrap_or_default();
            if matches!(agent.status(), AgentStatus::Online) && list.contains(&agent.peer_id()) {
                agents.push(agent.peer_id());
            }
        }
        Ok(agents)
    }
}

async fn process_command(
    ipfs: &Ipfs,
    keypair: &Keypair,
    agents: Arc<RwLock<HashSet<Agent>>>,
    responses: &mut HashMap<Uuid, futures::channel::oneshot::Sender<anyhow::Result<ResponseType>>>,
    event: ClientCommand<'_>,
) -> anyhow::Result<()> {
    match event {
        ClientCommand::Store {
            namespace,
            payload,
            response,
        } => {
            let agents = Vec::from_iter(agents.read().await.clone());
            if agents.is_empty() {
                let _ = response.send(Err(anyhow::anyhow!("No agents available")));
                anyhow::bail!("No agents available");
            }

            let bytes = payload.to_bytes()?;

            if bytes.len() > MAX_TRANSMIT_SIZE {
                let _ = response.send(Err(anyhow::anyhow!("Payload exceeds max transmission size")));
                anyhow::bail!("Payload exceeds max transmission size");
            }

            let request = construct_request(keypair, Identifier::Store, &namespace, None, payload)?;

            let request = Request::new(
                Identifier::Store,
                namespace.into(),
                None,
                Some(payload),
                signature.into(),
            );

            let request_id = request.id();

            let request_bytes = request.to_bytes()?;
            // TODO: Prioritize agents
            for agent in agents {
                //TODO: Check agent status
                //TODO: Check and confirm agent is subscribed to topic

                let bytes = ecdh_encrypt(keypair, Some(agent.public_key()), &request_bytes)?;

                ipfs.pubsub_publish(format!("/shuttle/request/{}", agent.peer_id()), bytes)
                    .await?;
            }

            responses.insert(request_id, response);
        }
        ClientCommand::Find { .. } => {}
        ClientCommand::Delete { .. } => {}
    }

    Ok(())
}

async fn process_message(
    keypair: &Keypair,
    message: Message,
    agents: Arc<RwLock<HashSet<Agent>>>,
    responses: &mut HashMap<Uuid, futures::channel::oneshot::Sender<anyhow::Result<ResponseType>>>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        message.data.len() < MAX_TRANSMIT_SIZE,
        "Message exceeded max transit size"
    );

    let Some(publickey) = agents.read().await.iter().find(|a| message.source == Some(a.peer_id())).map(|a| a.public_key().clone()) else {
        anyhow::bail!("Unable to find responding agent");
    };

    let resp_bytes = ecdh_decrypt(keypair, Some(&publickey), &message.data)?;

    let resp = Response::from_bytes(&resp_bytes)?;

    if let Err(e) = resp.verify(&keypair.public()) {
        if let Some(channel) = responses.remove(&resp.id()) {
            let _ = channel.send(Err(e));
        }
    } else if let Some(channel) = responses.remove(&resp.id()) {
        let _ = channel.send(Ok(ResponseType::ResponseOwned(message.source, resp_bytes)));
    }
    Ok(())
}

use std::collections::HashMap;
use std::{collections::HashSet, sync::Arc};

use crate::agent::{Agent, AgentStatus};
use crate::payload::Payload;
use crate::response::{Response, Status};
use futures::channel::oneshot;
use futures::StreamExt;
use rust_ipfs::{Ipfs, PeerId, PublicKey};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug)]
pub enum ClientCommand<'a> {
    Store {
        namespace: Vec<u8>,
        payload: Payload<'a>,
        response: oneshot::Receiver<ResponseType>,
    },
    Find {
        namespace: Vec<u8>,
        response: oneshot::Receiver<ResponseType>,
    },
    Delete {
        namespace: Vec<u8>,
        response: oneshot::Receiver<Status>,
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
            let _agents = agents.clone();
            let mut rx = rx;
            async move {
                let keypair = ipfs.keypair()?;
                let peer_id = keypair.public().to_peer_id();

                let response = ipfs
                    .pubsub_subscribe(format!("/shuttle/response/{peer_id}"))
                    .await?;

                let mut awaiting_response: HashMap<
                    Uuid,
                    futures::channel::oneshot::Sender<ResponseType>,
                > = HashMap::new();
                futures::pin_mut!(response);

                loop {
                    tokio::select! {
                        biased;
                        Some(command) = rx.next() => {
                            match command {
                                ClientCommand::Store { .. } => {},
                                ClientCommand::Find { .. } => {},
                                ClientCommand::Delete { .. } => {},
                            }
                        },
                        Some(response) = response.next() => {

                            if response.data.len() > 8 * 1024 * 1024 {
                                continue;
                            }
                            //TODO: Maybe process response here? 
                            let resp = match Response::from_bytes(&response.data) {
                                Ok(res) => res,
                                Err(_e) => {
                                    continue;
                                }
                            };

                            if let Some(channel) = awaiting_response.remove(&resp.id()) {
                                let _ = channel.send(ResponseType::ResponseOwned(response.source, response.data));
                            }
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
            agent.connect(&self.ipfs).await?;
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
        _agent: Option<PeerId>,
        _recipient: P,
        _namespace: N,
        _data: S,
    ) -> Result<(), anyhow::Error> {
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

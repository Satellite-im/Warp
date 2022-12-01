#![allow(clippy::await_holding_lock)]
use futures::StreamExt;
use ipfs::libp2p::gossipsub::GossipsubMessage;
use ipfs::{Ipfs, IpfsTypes, PeerId};
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::log::{error, warn};
use warp::multipass::identity::Message;

use libipld::IpldCodec;
use sata::{Kind, Sata};
use serde::{Deserialize, Serialize};
use warp::crypto::DID;
use warp::error::Error;
use warp::multipass::{MultiPassEventKind, PubsubMessage, SignedMessage};
use warp::sync::{Arc, RwLock};

use warp::tesseract::Tesseract;

use crate::config::Discovery;
use crate::store::connected_to_peer;
use crate::Persistent;
use warp::multipass::Signable;

use super::identity::IdentityStore;
use super::parsers::signaling::SignalingPayload;
use super::{did_keypair, did_to_libp2p_pub, libp2p_pub_to_did, PeerConnectionType};

pub fn get_signaling_topic(did: &DID) -> String {
    format!("/peer/{}/signaling", did)
}

pub struct WebrtcManager<T: IpfsTypes> {
    ipfs: Ipfs<T>,

    // Identity Store
    identity: IdentityStore<T>,

    // keypair
    did_key: Arc<DID>,

    // path to where things are stored (used for the queue)
    path: Option<PathBuf>,

    // Would be used to stop the look in the tokio task
    end_event: Arc<AtomicBool>,

    // Tesseract
    tesseract: Tesseract,

    // Used to broadcast request
    queue: Arc<RwLock<Vec<Queue>>>,

    tx: broadcast::Sender<MultiPassEventKind>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Hash, Eq)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum SignalingMessage {
    Offer(Message),
    Answer(Message),
    IceCandidate(Message),
}

impl Deref for SignalingMessage {
    type Target = Message;

    fn deref(&self) -> &Self::Target {
        match self {
            SignalingMessage::Offer(req) => req,
            SignalingMessage::Answer(req) => req,
            SignalingMessage::IceCandidate(req) => req,
        }
    }
}

impl<T: IpfsTypes> Clone for WebrtcManager<T> {
    fn clone(&self) -> Self {
        Self {
            ipfs: self.ipfs.clone(),
            identity: self.identity.clone(),
            did_key: self.did_key.clone(),
            end_event: self.end_event.clone(),
            tesseract: self.tesseract.clone(),
            tx: self.tx.clone(),
            queue: self.queue.clone(),
            path: self.path.clone(),
        }
    }
}

impl<T: IpfsTypes> WebrtcManager<T> {
    pub async fn new(
        ipfs: Ipfs<T>,
        identity: IdentityStore<T>,
        path: Option<PathBuf>,
        tesseract: Tesseract,
        interval: u64,
        tx: broadcast::Sender<MultiPassEventKind>,
    ) -> anyhow::Result<Self> {
        let path = match std::any::TypeId::of::<T>() == std::any::TypeId::of::<Persistent>() {
            true => path,
            false => None,
        };

        let end_event = Arc::new(AtomicBool::new(false));
        let queue = Arc::new(Default::default());
        let did_key = Arc::new(did_keypair(&tesseract)?);
        let did_copy = did_key.clone();

        let store = Self {
            ipfs,
            identity,
            path,
            did_key,
            end_event,
            tesseract,
            queue,
            tx,
        };

        let store_inner = store.clone();

        let stream = store
            .ipfs
            .pubsub_subscribe(get_signaling_topic(&did_copy))
            .await?;

        let (local_ipfs_public_key, _) = store.local().await?;

        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        tokio::spawn(async move {
            let mut store = store_inner;

            futures::pin_mut!(stream);
            let mut broadcast_interval = tokio::time::interval(Duration::from_millis(interval));

            loop {
                if store.end_event.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    message = stream.next() => {
                        if let Some(message) = message {
                            if let Err(e) = store.check_request_message(&local_public_key, message).await {
                                error!("Error: {e}");
                            }
                        }
                    }
                    _ = broadcast_interval.tick() => {
                        if let Err(e) = store.check_queue().await {
                            error!("Error: {e}");
                        }
                    }
                }
            }
        });
        tokio::task::yield_now().await;
        Ok(store)
    }

    //TODO: Implement Errors
    async fn check_request_message(
        &mut self,
        local_public_key: &DID,
        message: Arc<GossipsubMessage>,
    ) -> anyhow::Result<()> {
        if let Ok(data) = serde_json::from_slice::<Sata>(&message.data) {
            println!("Received: {:?}", data);
            let data = data.decrypt::<SignedMessage>(&self.did_key)?;

            println!("Received signaling message: {:#?}", data);

            if data.message.to.ne(local_public_key) {
                warn!("Request is not meant for identity. Skipping");
                return Ok(());
            }

            // first verify the request before processing it
            // validate_message(&data)?;
        }
        Ok(())
    }

    //TODO: Implement checks to determine if request been accepted, etc.
    async fn check_queue(&self) -> anyhow::Result<()> {
        let list = self.queue.read().clone();
        for item in list.iter() {
            let Queue(peer, data, recipient) = item;
            if let Ok(crate::store::PeerConnectionType::Connected) =
                connected_to_peer(self.ipfs.clone(), *peer).await
            {
                let bytes = serde_json::to_vec(&data)?;

                self.ipfs
                    .pubsub_publish(get_signaling_topic(recipient), bytes)
                    .await?;

                let index = self
                    .queue
                    .read()
                    .iter()
                    .position(|q| Queue(*peer, data.clone(), recipient.clone()).eq(q))
                    .ok_or_else(|| Error::OtherWithContext("Cannot find item in queue".into()))?;

                let _ = self.queue.write().remove(index);

                self.save_queue().await;
            }
        }
        Ok(())
    }

    async fn local(&self) -> anyhow::Result<(ipfs::libp2p::identity::PublicKey, PeerId)> {
        let (local_ipfs_public_key, local_peer_id) = self
            .ipfs
            .identity()
            .await
            .map(|(p, _)| (p.clone(), p.to_peer_id()))?;
        Ok((local_ipfs_public_key, local_peer_id))
    }
}

impl<T: IpfsTypes> WebrtcManager<T> {
    pub async fn send_signal(
        &mut self,
        pubkey: &DID,
        payload: &SignalingPayload,
    ) -> Result<(), Error> {
        println!("Sending signal to: {}", pubkey);

        let (local_ipfs_public_key, _) = self.local().await?;
        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotSendSelfFriendRequest);
        }

        let mut signaling_message: PubsubMessage<SignalingPayload> = PubsubMessage {
            from: local_public_key,
            to: pubkey.clone(),
            message_type: "signaling".into(),
            payload: payload.clone(),
        };

        let signed_message = signaling_message.sign(&self.tesseract)?;

        self.send_message(pubkey, &signed_message).await
    }

    pub async fn send_message(
        &mut self,
        pubkey: &DID,
        payload: &SignedMessage,
    ) -> Result<(), Error> {
        println!(
            "Sending message to: {}\n Signature: {:?} \n Message: {:?}",
            pubkey, payload.signature, payload.message
        );

        let (local_ipfs_public_key, _) = self.local().await?;
        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotSendSelfFriendRequest);
        }

        self.broadcast_message(&payload).await
    }
}

impl<T: IpfsTypes> WebrtcManager<T> {
    pub async fn broadcast_message(&mut self, signed_message: &SignedMessage) -> Result<(), Error> {
        let remote_peer_id = did_to_libp2p_pub(&signed_message.message.to)?.to_peer_id();

        if matches!(
            self.identity.discovery_type(),
            Discovery::Direct | Discovery::None
        ) {
            let peer_id = did_to_libp2p_pub(&signed_message.message.to)?.to_peer_id();

            let connected = super::connected_to_peer(self.ipfs.clone(), remote_peer_id).await?;

            if connected != PeerConnectionType::Connected {
                let res = match tokio::time::timeout(
                    Duration::from_secs(2),
                    self.ipfs.find_peer_info(peer_id),
                )
                .await
                {
                    Ok(res) => res,
                    Err(e) => Err(anyhow::anyhow!("{}", e.to_string())),
                };

                if let Err(_e) = res {
                    let ipfs = self.ipfs.clone();
                    let pubkey = signed_message.message.to.clone();
                    let relay = self.identity.relays();
                    let discovery = self.identity.discovery_type();
                    tokio::spawn(async move {
                        if let Err(e) = super::discover_peer(ipfs, &pubkey, discovery, relay).await
                        {
                            error!("Error discoverying peer: {e}");
                        }
                    });
                    tokio::task::yield_now().await;
                }
            }
        }

        let mut data = Sata::default();
        data.add_recipient(&signed_message.message.to)
            .map_err(anyhow::Error::from)?;
        let kp = &*self.did_key;
        let payload = data
            .encrypt(
                IpldCodec::DagJson,
                kp.as_ref(),
                Kind::Static,
                signed_message.clone(),
            )
            .map_err(anyhow::Error::from)?;
        let bytes = serde_json::to_vec(&payload)?;

        //Check to make sure the payload itself doesnt exceed 256kb
        if bytes.len() >= 256 * 1024 {
            return Err(Error::InvalidLength {
                context: "payload".into(),
                current: bytes.len(),
                minimum: Some(1),
                maximum: Some(256 * 1024),
            });
        }

        println!(
            "Going to broadcast : {}",
            get_signaling_topic(&signed_message.message.to)
        );

        let peers = self
            .ipfs
            .pubsub_peers(Some(get_signaling_topic(&signed_message.message.to)))
            .await?;

        if !peers.contains(&remote_peer_id) {
            self.queue.write().push(Queue(
                remote_peer_id,
                payload,
                signed_message.message.to.clone(),
            ));
            self.save_queue().await;
        } else if let Err(_e) = self
            .ipfs
            .pubsub_publish(get_signaling_topic(&signed_message.message.to), bytes)
            .await
        {
            self.queue.write().push(Queue(
                remote_peer_id,
                payload,
                signed_message.message.to.clone(),
            ));
            self.save_queue().await;
        }

        Ok(())
    }

    async fn save_queue(&self) {
        if let Some(path) = self.path.as_ref() {
            let bytes = match serde_json::to_vec(&self.queue) {
                Ok(bytes) => bytes,
                Err(e) => {
                    error!("Error serializing queue list into bytes: {e}");
                    return;
                }
            };

            if let Err(e) = tokio::fs::write(path.join(".request_queue"), bytes).await {
                error!("Error saving queue: {e}");
            }
        }
    }
}

fn validate_message(real_message: &SignalingMessage) -> Result<(), Error> {
    // let mut signaling_message = SignalingMessage::default();
    // signaling_message.set_from(real_message.from());
    // signaling_message.set_to(real_message.to());
    // signaling_message.set_date(real_message.date());
    // signaling_message.set_data(real_message.data());

    // let signature = match real_message.signature() {
    //     Some(s) => bs58::decode(s).into_vec()?,
    //     None => return Err(Error::InvalidSignature),
    // };

    // verify_serde_sig(real_message.from(), &signaling_message, &signature)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
struct Queue(PeerId, Sata, DID);

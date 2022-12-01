#![allow(clippy::await_holding_lock)]
use chrono::{DateTime, Utc};
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
use warp::multipass::{MultiPassEventKind, PubsubMessage};
use warp::sync::{Arc, RwLock};

use warp::tesseract::Tesseract;

use crate::config::Discovery;
use crate::store::{connected_to_peer, verify_serde_sig};
use crate::Persistent;

use super::identity::IdentityStore;
use super::parsers::signaling::SignalingPayload;
use super::{did_keypair, did_to_libp2p_pub, libp2p_pub_to_did, sign_serde, PeerConnectionType};

pub fn get_signaling_topic(did: &DID) -> String {
    format!("/peer/{}/signaling", did)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomPayload {
    pub is_offer: bool,
    pub data: String,
}

fn test() {
    let mut a: PubsubMessage<CustomPayload> = PubsubMessage {
        from: DID::default(),
        to: DID::default(),
        message_type: "test".to_string(),
        payload: CustomPayload {
            is_offer: true,
            data: "test".to_string(),
        },
    };

    if let Ok(b) = a.sign() {
        println!("{:?}", b);

        b.verify();
    }
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

// impl InternalRequest {
//     pub fn request_type(&self) -> InternalRequestType {
//         match self {
//             InternalRequest::In(_) => InternalRequestType::Incoming,
//             InternalRequest::Out(_) => InternalRequestType::Outgoing,
//         }
//     }
// }

// #[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
// pub enum InternalRequestType {
//     Incoming,
//     Outgoing,
// }

// impl InternalRequest {
//     pub fn valid(&self) -> Result<(), Error> {
//         let mut request = FriendRequest::default();
//         request.set_from(self.from());
//         request.set_to(self.to());
//         request.set_status(self.status());
//         request.set_date(self.date());

//         let signature = match self.signature() {
//             Some(s) => bs58::decode(s).into_vec()?,
//             None => return Err(Error::InvalidSignature),
//         };

//         verify_serde_sig(self.from(), &request, &signature)?;

//         Ok(())
//     }
// }

// pub struct SignalingMessage {
//     message: Message,
// }

// impl SignalingMessage {
//     pub fn new(from: DID, to: DID, payload: SignalingPayload) -> Self {
//         let message = Message::default();

//         let serialized_payload = serde_json::to_string(&payload).unwrap();

//         message.set_from(from);
//         message.set_to(to);
//         message.set_payload(serialized_payload);

//         let signature = bs58::encode(sign_serde(&self.tesseract, &request)?).into_string();

//         message.set_signature(signature);

//         Self { message }
//     }
// }

// #[cfg(not(target_arch = "wasm32"))]
// impl SignalingMessage {
//     pub fn set_date(&mut self, date: DateTime<Utc>) {
//         self.date = date
//     }

//     pub fn date(&self) -> DateTime<Utc> {
//         self.date
//     }
// }

// #[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Hash, Eq)]
// #[serde(rename_all = "lowercase", tag = "type")]
// pub enum InternalSignaling {
//     Offer(SignalingMessage),
//     Answer(SignalingMessage),
//     Candidate(SignalingMessage),
// }

// impl Deref for InternalSignaling {
//     type Target = SignalingMessage;

//     fn deref(&self) -> &Self::Target {
//         match self {
//             InternalSignaling::Offer(req) => req,
//             InternalSignaling::Answer(req) => req,
//             InternalSignaling::Candidate(req) => req,
//         }
//     }
// }

// impl InternalSignaling {
//     pub fn signal_type(&self) -> InternalSignalingType {
//         match self {
//             InternalSignaling::Offer(_) => InternalSignalingType::Offer,
//             InternalSignaling::Answer(_) => InternalSignalingType::Answer,
//             InternalSignaling::Candidate(_) => InternalSignalingType::Candidate,
//         }
//     }
// }

// #[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
// pub enum InternalSignalingType {
//     Offer,
//     Answer,
//     Candidate,
// }

// impl InternalSignaling {
//     pub fn valid(&self) -> Result<(), Error> {
//         let mut message = SignalingMessage::default();
//         message.set_from(self.from());
//         message.set_to(self.to());
//         message.set_data(self.data());
//         message.set_date(self.date());

//         let signature = match self.signature() {
//             Some(s) => bs58::decode(s).into_vec()?,
//             None => return Err(Error::InvalidSignature),
//         };

//         verify_serde_sig(self.from(), &message, &signature)?;

//         Ok(())
//     }
// }

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
            let data = data.decrypt::<SignalingMessage>(&self.did_key)?;

            println!("Received signaling message: {:#?}", data);

            if data.to().ne(local_public_key) {
                warn!("Request is not meant for identity. Skipping");
                return Ok(());
            }

            // first verify the request before processing it
            validate_message(&data)?;
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
    pub async fn send_signal(&mut self, pubkey: &DID, payload: &String) -> Result<(), Error> {
        println!("Sending signal to: {}", pubkey);

        let (local_ipfs_public_key, _) = self.local().await?;
        let local_public_key = libp2p_pub_to_did(&local_ipfs_public_key)?;

        if local_public_key.eq(pubkey) {
            return Err(Error::CannotSendSelfFriendRequest);
        }

        Ok(())

        // let mut signaling_message = SignalingMessage::default();
        // signaling_message.set_from(local_public_key);
        // signaling_message.set_to(pubkey.clone());
        // signaling_message.set_data(payload.clone());
        // let signature =
        //     bs58::encode(sign_serde(&self.tesseract, &signaling_message)?).into_string();

        // signaling_message.set_signature(signature);

        // self.broadcast_message(&signaling_message).await
    }
}

impl<T: IpfsTypes> WebrtcManager<T> {
    pub async fn broadcast_message(&mut self, request: &SignalingMessage) -> Result<(), Error> {
        let remote_peer_id = did_to_libp2p_pub(&request.to())?.to_peer_id();

        if matches!(
            self.identity.discovery_type(),
            Discovery::Direct | Discovery::None
        ) {
            let peer_id = did_to_libp2p_pub(&request.to())?.to_peer_id();

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
                    let pubkey = request.to();
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
        data.add_recipient(&request.to())
            .map_err(anyhow::Error::from)?;
        let kp = &*self.did_key;
        let payload = data
            .encrypt(
                IpldCodec::DagJson,
                kp.as_ref(),
                Kind::Static,
                request.clone(),
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

        let peers = self
            .ipfs
            .pubsub_peers(Some(get_signaling_topic(&request.to())))
            .await?;

        if !peers.contains(&remote_peer_id) {
            self.queue
                .write()
                .push(Queue(remote_peer_id, payload, request.to()));
            self.save_queue().await;
        } else if let Err(_e) = self
            .ipfs
            .pubsub_publish(get_signaling_topic(&request.to()), bytes)
            .await
        {
            self.queue
                .write()
                .push(Queue(remote_peer_id, payload, request.to()));
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

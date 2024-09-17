use crate::store::conversation::ConversationDocument;
use crate::store::document::root::RootDocumentMap;
use crate::store::files::FileStore;
use crate::store::identity::IdentityStore;
use crate::store::keystore::Keystore;
use crate::store::message::{ConversationInner, ConversationStreamData};
use crate::store::payload::{PayloadBuilder, PayloadMessage};
use crate::store::topics::PeerTopic;
use crate::store::{
    ecdh_decrypt, ecdh_encrypt, ecdh_shared_key, ConversationEvents, ConversationRequestKind,
    ConversationRequestResponse, ConversationResponseKind, DidExt, MessagingEvents, PeerIdExt,
};
use chrono::{DateTime, Utc};
use futures::channel::mpsc::Receiver;
use futures::channel::oneshot::Sender;
use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt};
use indexmap::IndexSet;
use libipld::Cid;
use rust_ipfs::libp2p::gossipsub::Message;
use rust_ipfs::Ipfs;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tracing::{error, warn};
use uuid::Uuid;
use warp::crypto::cipher::Cipher;
use warp::crypto::{generate, DID};
use warp::error::Error;
use warp::raygun::ConversationType;
use web_time::Instant;

pub struct MainConversationTask {
    conversation_id: Uuid,
    root: RootDocumentMap,
    identity: IdentityStore,
    file: FileStore,
    ipfs: Ipfs,
    conversation_type: ConversationType,
    participants: IndexSet<DID>,
    pending_messages: BTreeSet<PendingMessage>,
    pending_key_exchange: HashMap<Uuid, Vec<(DID, Vec<u8>, bool)>>,
    stream: BoxStream<'static, ConversationStreamData>,
}

impl MainConversationTask {
    pub async fn new(
        conversation_id: Uuid,
        ipfs: &Ipfs,
        root: &RootDocumentMap,
        file: &FileStore,
        identity: &IdentityStore,
        conversation_type: ConversationType,
    ) -> Result<Self, Error> {
        let mut task = MainConversationTask {
            conversation_id,
            conversation_type,
            identity: identity.clone(),
            file: file.clone(),
            root: root.clone(),
            ipfs: ipfs.clone(),
            participants: Default::default(),
            pending_messages: Default::default(),
            pending_key_exchange: Default::default(),
            stream: futures::stream::empty().boxed(),
        };

        let conversation = task.get_document().await?;

        let main_topic = conversation.topic();
        let event_topic = conversation.event_topic();
        let request_topic = conversation.exchange_topic(&identity.did_key());

        let messaging_stream = ipfs
            .pubsub_subscribe(main_topic)
            .await?
            .map(move |msg| ConversationStreamData::Message(conversation_id, msg))
            .boxed();

        let event_stream = ipfs
            .pubsub_subscribe(event_topic)
            .await?
            .map(move |msg| ConversationStreamData::Event(conversation_id, msg))
            .boxed();

        let request_stream = ipfs
            .pubsub_subscribe(request_topic)
            .await?
            .map(move |msg| ConversationStreamData::RequestResponse(conversation_id, msg))
            .boxed();

        let stream = futures::stream::select_all([messaging_stream, event_stream, request_stream]);

        task.stream = stream.boxed();

        Ok(task)
    }

    pub async fn run(mut self) {
        let this = &mut self;
        loop {}
    }
}

impl MainConversationTask {
    async fn process_request_response_event(
        &mut self,
        conversation_id: Uuid,
        req: Message,
    ) -> Result<(), Error> {
        let keypair = self.root.keypair();
        let own_did = self.identity.did_key();

        let conversation = self.get_document().await?;

        let payload = PayloadMessage::<Vec<u8>>::from_bytes(&req.data)?;

        let sender = payload.sender().to_did()?;

        let data = ecdh_decrypt(keypair, Some(&sender), payload.message())?;

        let event = serde_json::from_slice::<ConversationRequestResponse>(&data)?;

        tracing::debug!(%conversation_id, ?event, "Event received");
        match event {
            ConversationRequestResponse::Request {
                conversation_id,
                kind,
            } => match kind {
                ConversationRequestKind::Key => {
                    if !matches!(conversation.conversation_type(), ConversationType::Group) {
                        //Only group conversations support keys
                        return Err(Error::InvalidConversation);
                    }

                    if !conversation.recipients().contains(&sender) {
                        warn!(%conversation_id, %sender, "apart of conversation");
                        return Err(Error::IdentityDoesntExist);
                    }

                    let mut keystore = self.get_keystore().await?;

                    let raw_key = match keystore.get_latest(keypair, &own_did) {
                        Ok(key) => key,
                        Err(Error::PublicKeyDoesntExist) => {
                            let key = generate::<64>().into();
                            keystore.insert(keypair, &own_did, &key)?;

                            this.set_keystore(conversation_id, keystore).await?;
                            key
                        }
                        Err(e) => {
                            error!(%conversation_id, error = %e, "Error getting key from store");
                            return Err(e);
                        }
                    };

                    let key = ecdh_encrypt(keypair, Some(&sender), raw_key)?;

                    let response = ConversationRequestResponse::Response {
                        conversation_id,
                        kind: ConversationResponseKind::Key { key },
                    };

                    let topic = conversation.exchange_topic(&sender);

                    let bytes =
                        ecdh_encrypt(keypair, Some(&sender), serde_json::to_vec(&response)?)?;

                    let payload = PayloadBuilder::new(keypair, bytes)
                        .from_ipfs(&self.ipfs)
                        .await?;

                    let peers = self.ipfs.pubsub_peers(Some(topic.clone())).await?;

                    let peer_id = sender.to_peer_id()?;

                    let bytes = payload.to_bytes()?;

                    tracing::trace!(%conversation_id, "Payload size: {} bytes", bytes.len());

                    tracing::info!(%conversation_id, "Responding to {sender}");

                    if !peers.contains(&peer_id)
                        || (peers.contains(&peer_id)
                            && self
                                .ipfs
                                .pubsub_publish(topic.clone(), bytes)
                                .await
                                .is_err())
                    {
                        warn!(%conversation_id, "Unable to publish to topic. Queuing event");
                        // this.queue_event(
                        //     sender.clone(),
                        //     Queue::direct(
                        //         conversation_id,
                        //         None,
                        //         peer_id,
                        //         topic.clone(),
                        //         payload.message().to_vec(),
                        //     ),
                        // )
                        // .await;
                    }
                }
                _ => {
                    tracing::info!(%conversation_id, "Unimplemented/Unsupported Event");
                }
            },
            ConversationRequestResponse::Response {
                conversation_id,
                kind,
            } => match kind {
                ConversationResponseKind::Key { key } => {
                    if !matches!(conversation.conversation_type(), ConversationType::Group) {
                        //Only group conversations support keys
                        tracing::error!(%conversation_id, "Invalid conversation type");
                        return Err(Error::InvalidConversation);
                    }

                    if !conversation.recipients().contains(&sender) {
                        return Err(Error::IdentityDoesntExist);
                    }
                    let mut keystore = self.get_keystore().await?;

                    let raw_key = ecdh_decrypt(keypair, Some(&sender), key)?;

                    keystore.insert(keypair, &sender, raw_key)?;

                    self.set_keystore(keystore).await?;

                    if let Some(list) = this.pending_key_exchange.get_mut(&conversation_id) {
                        for (_, _, received) in
                            list.iter_mut().filter(|(s, _, r)| sender.eq(s) && !r)
                        {
                            *received = true;
                        }
                    }
                }
                _ => {
                    tracing::info!(%conversation_id, "Unimplemented/Unsupported Event");
                }
            },
        }
        Ok(())
    }

    pub async fn publish(
        &mut self,
        message_id: Option<Uuid>,
        event: MessagingEvents,
        queue: bool,
    ) -> Result<(), Error> {
        let conversation = self.get_document().await?;
        let event = serde_json::to_vec(&event)?;
        let keypair = self.root.keypair();
        let own_did = self.identity.did_key();

        let key = self.conversation_key(None).await?;

        let bytes = Cipher::direct_encrypt(&event, &key)?;

        let payload = PayloadBuilder::new(keypair, bytes)
            .from_ipfs(&self.ipfs)
            .await?;

        let peers = self.ipfs.pubsub_peers(Some(conversation.topic())).await?;

        let mut can_publish = false;

        for recipient in self.participants.iter().filter(|did| own_did.ne(did)) {
            let peer_id = recipient.to_peer_id()?;

            // We want to confirm that there is atleast one peer subscribed before attempting to send a message
            match peers.contains(&peer_id) {
                true => {
                    can_publish = true;
                }
                false => {
                    if queue {
                        // self.queue_event(
                        //     recipient.clone(),
                        //     Queue::direct(
                        //         conversation.id(),
                        //         message_id,
                        //         peer_id,
                        //         conversation.topic(),
                        //         payload.message().to_vec(),
                        //     ),
                        // )
                        // .await;
                    }
                }
            };
        }

        if can_publish {
            let bytes = payload.to_bytes()?;
            tracing::trace!(id=%self.conversation_id, "Payload size: {} bytes", bytes.len());
            let timer = Instant::now();
            let mut time = true;
            if let Err(_e) = self.ipfs.pubsub_publish(conversation.topic(), bytes).await {
                tracing::error!(id=%self.conversation_id, "Error publishing: {_e}");
                time = false;
            }
            if time {
                let end = timer.elapsed();
                tracing::trace!(id=%self.conversation_id, "Took {}ms to send event", end.as_millis());
            }
        }

        Ok(())
    }

    async fn send_single_conversation_event(
        &mut self,
        did_key: &DID,
        event: ConversationEvents,
    ) -> Result<(), Error> {
        let event = serde_json::to_vec(&event)?;

        let keypair = self.root.keypair();

        let bytes = ecdh_encrypt(keypair, Some(did_key), &event)?;

        let payload = PayloadBuilder::new(keypair, bytes)
            .from_ipfs(&self.ipfs)
            .await?;

        let peer_id = did_key.to_peer_id()?;
        let peers = self.ipfs.pubsub_peers(Some(did_key.messaging())).await?;

        let mut time = true;
        let timer = Instant::now();
        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(did_key.messaging(), payload.to_bytes()?)
                    .await
                    .is_err())
        {
            tracing::warn!(id=%self.conversation_id, "Unable to publish to topic. Queuing event");
            // self.queue_event(
            //     did_key.clone(),
            //     Queue::direct(
            //         self.conversation_id,
            //         None,
            //         peer_id,
            //         did_key.messaging(),
            //         payload.message().to_vec(),
            //     ),
            // )
            // .await;
            time = false;
        }
        if time {
            let end = timer.elapsed();
            tracing::info!(%self.conversation_id, "Event sent to {did_key}");
            tracing::trace!(%self.conversation_id, "Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    async fn conversation_key(&self, did: Option<DID>) -> Result<Vec<u8>, Error> {
        let keypair = self.root.keypair();
        let own_did = self.identity.did_key();

        match self.conversation_type {
            ConversationType::Direct => {
                let recipients = self
                    .participants
                    .iter()
                    .filter(|did| own_did.ne(did))
                    .collect::<IndexSet<_>>();

                let member = recipients.first().ok_or(Error::InvalidConversation)?;
                ecdh_shared_key(keypair, Some(member))
            }
            ConversationType::Group => {
                let keystore = self
                    .root
                    .get_conversation_keystore(self.conversation_id)
                    .await?;
                let recipient = did.unwrap_or(own_did);
                keystore.get_latest(keypair, &recipient)
            }
        }
    }

    async fn get_document(&self) -> Result<ConversationDocument, Error> {
        self.root
            .get_conversation_document(self.conversation_id)
            .await
    }

    async fn get_keystore(&self) -> Result<Keystore, Error> {
        self.root
            .get_conversation_keystore(self.conversation_id)
            .await
    }

    async fn set_keystore(&mut self, document: Keystore) -> Result<(), Error> {
        let mut map = self.root.get_conversation_keystore_map().await?;

        let id = self.conversation_id.to_string();
        let cid = self.ipfs.dag().put().serialize(document).await?;

        map.insert(id, cid);

        self.set_keystore_map(map).await
    }

    pub async fn set_keystore_map(&mut self, map: BTreeMap<String, Cid>) -> Result<(), Error> {
        self.root.set_conversation_keystore_map(map).await
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum PendingKind {
    Add,
    Remove,
    Modified,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct PendingMessage {
    date: DateTime<Utc>,
    pending_kind: PendingKind,
    message: (),
}

// impl PartialOrd for PendingMessage {
//     fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {}
// }

async fn process_request_response_event(
    this: &mut ConversationInner,
    conversation_id: Uuid,
    req: Message,
) -> Result<(), Error> {
    let keypair = &this.root.keypair().clone();
    let own_did = this.identity.did_key();

    let conversation = this.get(conversation_id).await?;

    let payload = PayloadMessage::<Vec<u8>>::from_bytes(&req.data)?;

    let sender = payload.sender().to_did()?;

    let data = ecdh_decrypt(keypair, Some(&sender), payload.message())?;

    let event = serde_json::from_slice::<ConversationRequestResponse>(&data)?;

    tracing::debug!(%conversation_id, ?event, "Event received");
    match event {
        ConversationRequestResponse::Request {
            conversation_id,
            kind,
        } => match kind {
            ConversationRequestKind::Key => {
                if !matches!(conversation.conversation_type(), ConversationType::Group) {
                    //Only group conversations support keys
                    return Err(Error::InvalidConversation);
                }

                if !conversation.recipients().contains(&sender) {
                    warn!(%conversation_id, %sender, "apart of conversation");
                    return Err(Error::IdentityDoesntExist);
                }

                let mut keystore = this.get_keystore(conversation_id).await?;

                let raw_key = match keystore.get_latest(keypair, &own_did) {
                    Ok(key) => key,
                    Err(Error::PublicKeyDoesntExist) => {
                        let key = generate::<64>().into();
                        keystore.insert(keypair, &own_did, &key)?;

                        this.set_keystore(conversation_id, keystore).await?;
                        key
                    }
                    Err(e) => {
                        error!(%conversation_id, error = %e, "Error getting key from store");
                        return Err(e);
                    }
                };

                let key = ecdh_encrypt(keypair, Some(&sender), raw_key)?;

                let response = ConversationRequestResponse::Response {
                    conversation_id,
                    kind: ConversationResponseKind::Key { key },
                };

                let topic = conversation.exchange_topic(&sender);

                let bytes = ecdh_encrypt(keypair, Some(&sender), serde_json::to_vec(&response)?)?;

                let payload = PayloadBuilder::new(keypair, bytes)
                    .from_ipfs(&this.ipfs)
                    .await?;

                let peers = this.ipfs.pubsub_peers(Some(topic.clone())).await?;

                let peer_id = sender.to_peer_id()?;

                let bytes = payload.to_bytes()?;

                tracing::trace!(%conversation_id, "Payload size: {} bytes", bytes.len());

                tracing::info!(%conversation_id, "Responding to {sender}");

                if !peers.contains(&peer_id)
                    || (peers.contains(&peer_id)
                        && this
                            .ipfs
                            .pubsub_publish(topic.clone(), bytes)
                            .await
                            .is_err())
                {
                    warn!(%conversation_id, "Unable to publish to topic. Queuing event");
                    this.queue_event(
                        sender.clone(),
                        Queue::direct(
                            conversation_id,
                            None,
                            peer_id,
                            topic.clone(),
                            payload.message().to_vec(),
                        ),
                    )
                    .await;
                }
            }
            _ => {
                tracing::info!(%conversation_id, "Unimplemented/Unsupported Event");
            }
        },
        ConversationRequestResponse::Response {
            conversation_id,
            kind,
        } => match kind {
            ConversationResponseKind::Key { key } => {
                if !matches!(conversation.conversation_type(), ConversationType::Group) {
                    //Only group conversations support keys
                    tracing::error!(%conversation_id, "Invalid conversation type");
                    return Err(Error::InvalidConversation);
                }

                if !conversation.recipients().contains(&sender) {
                    return Err(Error::IdentityDoesntExist);
                }
                let mut keystore = this.get_keystore(conversation_id).await?;

                let raw_key = ecdh_decrypt(keypair, Some(&sender), key)?;

                keystore.insert(keypair, &sender, raw_key)?;

                this.set_keystore(conversation_id, keystore).await?;

                if let Some(list) = this.pending_key_exchange.get_mut(&conversation_id) {
                    for (_, _, received) in list.iter_mut().filter(|(s, _, r)| sender.eq(s) && !r) {
                        *received = true;
                    }
                }
            }
            _ => {
                tracing::info!(%conversation_id, "Unimplemented/Unsupported Event");
            }
        },
    }
    Ok(())
}

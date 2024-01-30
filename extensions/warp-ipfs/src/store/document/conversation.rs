use std::{
    collections::{
        btree_map::Entry as BTreeEntry, hash_map::Entry as HashEntry, BTreeMap, HashMap, HashSet
    },
    future::IntoFuture,
    path::PathBuf,
    sync::Arc,
};

use futures::{
    channel::{mpsc, oneshot},
    pin_mut,
    stream::FuturesUnordered,
    SinkExt, Stream, StreamExt,
};
use libipld::Cid;
use rust_ipfs::{libp2p::gossipsub::Message, Ipfs, IpfsPath};
use tokio::select;
use tokio_stream::StreamMap;
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::{error, warn};
use uuid::Uuid;
use warp::{
    crypto::{cipher::Cipher, generate, DID},
    error::Error,
    raygun::{
        Conversation, ConversationType, DirectConversationSettings, GroupSettings, MessageEventKind, PinState, ReactionState
    },
};

use crate::store::{
    conversation::{ConversationDocument, MessageDocument},
    ecdh_decrypt, ecdh_encrypt, generate_shared_topic,
    keystore::Keystore,
    message::{EventOpt, MessageDirection},
    payload::Payload,
    sign_serde, verify_serde_sig, ConversationEvents, ConversationRequestKind,
    ConversationRequestResponse, ConversationResponseKind, ConversationUpdateKind, DidExt,
    MessagingEvents, PeerTopic,
};

use super::root::RootDocumentMap;

#[allow(clippy::large_enum_variant)]
enum ConversationCommand {
    GetDocument {
        id: Uuid,
        response: oneshot::Sender<Result<ConversationDocument, Error>>,
    },
    GetKeystore {
        id: Uuid,
        response: oneshot::Sender<Result<Keystore, Error>>,
    },
    SetDocument {
        document: ConversationDocument,
        response: oneshot::Sender<Result<(), Error>>,
    },
    SetKeystore {
        id: Uuid,
        document: Keystore,
        response: oneshot::Sender<Result<(), Error>>,
    },
    Delete {
        id: Uuid,
        response: oneshot::Sender<Result<ConversationDocument, Error>>,
    },
    Contains {
        id: Uuid,
        response: oneshot::Sender<Result<bool, Error>>,
    },
    List {
        response: oneshot::Sender<Result<Vec<ConversationDocument>, Error>>,
    },
    Subscribe {
        id: Uuid,
        response: oneshot::Sender<Result<tokio::sync::broadcast::Sender<MessageEventKind>, Error>>,
    },
}

#[derive(Debug, Clone)]
pub struct Conversations {
    tx: mpsc::Sender<ConversationCommand>,
    _task_cancellation: Arc<DropGuard>,
}

impl Conversations {
    pub async fn new(
        ipfs: &Ipfs,
        path: Option<PathBuf>,
        keypair: Arc<DID>,
        root: RootDocumentMap,
    ) -> Self {
        let cid = match path.as_ref() {
            Some(path) => tokio::fs::read(path.join(".message_id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .ok()
                .and_then(|cid_str| cid_str.parse().ok()),
            None => None,
        };

        let (tx, rx) = futures::channel::mpsc::channel(0);

        let token = CancellationToken::new();
        let drop_guard = token.clone().drop_guard();
        let ipfs = ipfs.clone();
        tokio::spawn(async move {
            let topic_stream = StreamMap::new();
            let mut task = ConversationTask {
                ipfs,
                event_handler: Default::default(),
                keypair,
                path,
                cid,
                topic_stream,
                rx,
                root,
            };

            select! {
                _ = token.cancelled() => {}
                _ = task.run() => {}
            }
        });

        Self {
            tx,
            _task_cancellation: Arc::new(drop_guard),
        }
    }

    pub async fn get(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::GetDocument { id, response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_keystore(&self, id: Uuid) -> Result<Keystore, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::GetKeystore { id, response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn contains(&self, id: Uuid) -> Result<bool, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::Contains { id, response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn set(&self, document: ConversationDocument) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::SetDocument {
                document,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn set_keystore(&self, id: Uuid, document: Keystore) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::SetKeystore {
                id,
                document,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn delete(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::Delete { id, response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn list(&self) -> Result<Vec<ConversationDocument>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::List { response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn subscribe(
        &self,
        id: Uuid,
    ) -> Result<tokio::sync::broadcast::Sender<MessageEventKind>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::Subscribe { id, response: tx })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }
}

struct ConversationTask {
    ipfs: Ipfs,
    cid: Option<Cid>,
    path: Option<PathBuf>,
    keypair: Arc<DID>,
    event_handler: HashMap<Uuid, tokio::sync::broadcast::Sender<MessageEventKind>>,
    topic_stream: StreamMap<Uuid, mpsc::Receiver<ConversationStreamData>>,
    root: RootDocumentMap,
    rx: mpsc::Receiver<ConversationCommand>,
}

impl ConversationTask {
    async fn run(&mut self) {
        let stream = self
            .ipfs
            .pubsub_subscribe(self.keypair.messaging())
            .await
            .expect("valid subscription");

        pin_mut!(stream);

        loop {
            tokio::select! {
                Some(command) = self.rx.next() => {
                    match command {
                        ConversationCommand::GetDocument { id, response } => {
                            let _ = response.send(self.get(id).await);
                        }
                        ConversationCommand::SetDocument { document, response } => {
                            let _ = response.send(self.set_document(document).await);
                        }
                        ConversationCommand::List { response } => {
                            let _ = response.send(Ok(self.list().await));
                        }
                        ConversationCommand::Delete { id, response } => {
                            let _ = response.send(self.delete(id).await);
                        }
                        ConversationCommand::Subscribe { id, response } => {
                            let _ = response.send(self.subscribe(id).await);
                        }
                        ConversationCommand::Contains { id, response } => {
                            let _ = response.send(Ok(self.contains(id).await));
                        }
                        ConversationCommand::GetKeystore { id, response } => {
                            let _ = response.send(self.get_keystore(id).await);
                        }
                        ConversationCommand::SetKeystore {
                            id,
                            document,
                            response,
                        } => {
                            let _ = response.send(self.set_keystore(id, document).await);
                        }
                    }
                }
                Some(message) = stream.next() => {
                    let payload = match Payload::from_bytes(&message.data) {
                        Ok(payload) => payload,
                        Err(e) => {
                            tracing::warn!("Failed to parse payload data: {e}");
                            continue;
                        }
                    };

                    let data = match ecdh_decrypt(&self.keypair, Some(&payload.sender()), payload.data()) {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!("Failed to decrypt message from {}: {e}", payload.sender());
                            continue;
                        }
                    };

                    let events = match serde_json::from_slice::<ConversationEvents>(&data) {
                        Ok(ev) => ev,
                        Err(e) => {
                            tracing::warn!("Failed to parse message: {e}");
                            continue;
                        }
                    };

                    if let Err(e) = process_conversation(self, payload, events).await {
                        tracing::error!("Error processing conversation: {e}");
                    }
                }
                Some((conversation_id, message)) = self.topic_stream.next() => {
                    match message {
                        ConversationStreamData::RequestResponse(req) => {
                            let source = req.source;
                            if let Err(e) = process_request_response_event(self, conversation_id, req).await {
                                tracing::error!(id = %conversation_id, sender = ?source, error = %e, "Failed to process payload");
                            }
                        },
                        ConversationStreamData::Event(ev) => {
                            let source = ev.source;
                            if let Err(e) = process_conversation_event(self, conversation_id, ev).await {
                                tracing::error!(id = %conversation_id, sender = ?source, error = %e, "Failed to process payload");
                            }
                        },
                        ConversationStreamData::Message(msg) => {
                            let source = msg.source;
                            if let Err(e) = self.process_msg_event(conversation_id, msg).await {
                                tracing::error!(id = %conversation_id, sender = ?source, error = %e, "Failed to process payload");
                            }
                        },
                    }
                }
            }
        }
    }

    //TODO: Use function
    #[allow(dead_code)]
    async fn create_conversation(&mut self, did: &DID) -> Result<Conversation, Error> {
        //TODO: maybe use root document to directly check
        // if self.with_friends.load(Ordering::SeqCst) && !self.identity.is_friend(did_key).await? {
        //     return Err(Error::FriendDoesntExist);
        // }

        // if let Ok(true) = self.identity.is_blocked(did_key).await {
        //     return Err(Error::PublicKeyIsBlocked);
        // }

        if did == &*self.keypair {
            return Err(Error::CannotCreateConversation);
        }

        if let Some(conversation) = self
            .list()
            .await
            .iter()
            .find(|conversation| {
                conversation.conversation_type == ConversationType::Direct
                    && conversation.recipients().contains(did)
                    && conversation.recipients().contains(&self.keypair)
            })
            .cloned()
        {
            return Err(Error::ConversationExist {
                conversation: conversation.into(),
            });
        }

        //Temporary limit
        // if self.list_conversations().await.unwrap_or_default().len() >= 256 {
        //     return Err(Error::ConversationLimitReached);
        // }

        // if !self.discovery.contains(did_key).await {
        //     self.discovery.insert(did_key).await?;
        // }

        let settings = DirectConversationSettings::default();
        let conversation = ConversationDocument::new_direct(
            &self.keypair,
            [(*self.keypair).clone(), did.clone()],
            settings,
        )?;

        let convo_id = conversation.id();
        let main_topic = conversation.topic();
        let event_topic = conversation.event_topic();
        let request_topic = conversation.reqres_topic(&self.keypair);

        self.set_document(conversation.clone()).await?;

        let messaging_stream = self
            .ipfs
            .pubsub_subscribe(main_topic)
            .await?
            .map(ConversationStreamData::Message)
            .boxed();

        let event_stream = self
            .ipfs
            .pubsub_subscribe(event_topic)
            .await?
            .map(ConversationStreamData::Event)
            .boxed();

        let request_stream = self
            .ipfs
            .pubsub_subscribe(request_topic)
            .await?
            .map(ConversationStreamData::RequestResponse)
            .boxed();

        let mut stream = messaging_stream
            .chain(event_stream)
            .chain(request_stream)
            .boxed();

        let (mut tx, rx) = mpsc::channel(256);

        tokio::spawn(async move {
            while let Some(stream_type) = stream.next().await {
                if let Err(e) = tx.send(stream_type).await {
                    if e.is_disconnected() {
                        break;
                    }
                }
            }
        });

        self.topic_stream.insert(convo_id, rx);

        let peer_id = did.to_peer_id()?;

        let event = ConversationEvents::NewConversation {
            recipient: (*self.keypair).clone(),
            settings,
        };

        let bytes = ecdh_encrypt(&self.keypair, Some(did), serde_json::to_vec(&event)?)?;
        let signature = sign_serde(&self.keypair, &bytes)?;

        let payload = Payload::new(&self.keypair, &bytes, &signature);

        let peers = self.ipfs.pubsub_peers(Some(did.messaging())).await?;

        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(did.messaging(), payload.to_bytes()?.into())
                    .await
                    .is_err())
        {
            warn!("Unable to publish to topic. Queuing event");
            // if let Err(e) = self
            //     .queue_event(
            //         did_key.clone(),
            //         Queue::direct(
            //             convo_id,
            //             None,
            //             peer_id,
            //             did_key.messaging(),
            //             payload.data().to_vec(),
            //         ),
            //     )
            //     .await
            // {
            //     error!("Error submitting event to queue: {e}");
            // }
        }

        // self.event
        //     .emit(RayGunEventKind::ConversationCreated {
        //         conversation_id: convo_id,
        //     })
        //     .await;

        Ok(Conversation::from(&conversation))
    }

    pub async fn create_group_conversation(
        &mut self,
        name: Option<String>,
        mut recipients: HashSet<DID>,
        settings: GroupSettings,
    ) -> Result<Conversation, Error> {
        let own_did = &*(self.keypair.clone());

        if recipients.contains(own_did) {
            return Err(Error::CannotCreateConversation);
        }

        if let Some(name) = name.as_ref() {
            let name_length = name.trim().len();

            if name_length == 0 || name_length > 255 {
                return Err(Error::InvalidLength {
                    context: "name".into(),
                    current: name_length,
                    minimum: Some(1),
                    maximum: Some(255),
                });
            }
        }

        let mut removal = vec![];

        // for did in recipients.iter() {
        //     if self.with_friends.load(Ordering::SeqCst) && !self.identity.is_friend(did).await? {
        //         info!("{did} is not on the friends list.. removing from list");
        //         removal.push(did.clone());
        //     }

        //     if let Ok(true) = self.identity.is_blocked(did).await {
        //         info!("{did} is blocked.. removing from list");
        //         removal.push(did.clone());
        //     }
        // }

        for did in removal {
            recipients.remove(&did);
        }

        //Temporary limit
        // if self.list_conversations().await.unwrap_or_default().len() >= 256 {
        //     return Err(Error::ConversationLimitReached);
        // }

        // for recipient in &recipients {
        //     if !self.discovery.contains(recipient).await {
        //         let _ = self.discovery.insert(recipient).await.ok();
        //     }
        // }

        let restricted = self.root.get_blocks().await.unwrap_or_default();

        let conversation = ConversationDocument::new_group(
            own_did,
            name,
            &Vec::from_iter(recipients),
            &restricted,
            settings,
        )?;

        let recipient = conversation.recipients();

        let convo_id = conversation.id();
        let topic = conversation.topic();

        self.set_document(conversation).await?;

        let mut keystore = Keystore::new(convo_id);
        keystore.insert(own_did, own_did, warp::crypto::generate::<64>())?;

        self.set_keystore(convo_id, keystore).await?;

        // let stream = self.ipfs.pubsub_subscribe(topic).await?;

        // self.start_task(convo_id, stream).await;

        let peer_id_list = recipient
            .clone()
            .iter()
            .filter(|did| own_did.ne(did))
            .map(|did| (did.clone(), did))
            .filter_map(|(a, b)| b.to_peer_id().map(|pk| (a, pk)).ok())
            .collect::<Vec<_>>();

        let conversation = self.get(convo_id).await?;

        let event = serde_json::to_vec(&ConversationEvents::NewGroupConversation {
            conversation: conversation.clone(),
        })?;

        for (did, peer_id) in peer_id_list {
            let bytes = ecdh_encrypt(own_did, Some(&did), &event)?;
            let signature = sign_serde(own_did, &bytes)?;

            let payload = Payload::new(own_did, &bytes, &signature);

            let peers = self.ipfs.pubsub_peers(Some(did.messaging())).await?;
            if !peers.contains(&peer_id)
                || (peers.contains(&peer_id)
                    && self
                        .ipfs
                        .pubsub_publish(did.messaging(), payload.to_bytes()?.into())
                        .await
                        .is_err())
            {
                warn!("Unable to publish to topic. Queuing event");
                // if let Err(e) = self
                //     .queue_event(
                //         did.clone(),
                //         Queue::direct(
                //             convo_id,
                //             None,
                //             peer_id,
                //             did.messaging(),
                //             payload.data().to_vec(),
                //         ),
                //     )
                //     .await
                // {
                //     error!("Error submitting event to queue: {e}");
                // }
            }
        }

        for recipient in recipient.iter().filter(|d| own_did.ne(d)) {
            if let Err(e) = self.request_key(conversation.id(), recipient).await {
                tracing::warn!("Failed to send exchange request to {recipient}: {e}");
            }
        }

        // self.event
        //     .emit(RayGunEventKind::ConversationCreated {
        //         conversation_id: conversation.id(),
        //     })
        //     .await;

        Ok(Conversation::from(&conversation))
    }

    async fn get(&self, id: Uuid) -> Result<ConversationDocument, Error> {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return Err(Error::InvalidConversation),
        };

        let path = IpfsPath::from(cid).sub_path(&id.to_string())?;

        let document: ConversationDocument =
            match self.ipfs.get_dag(path).local().deserialized().await {
                Ok(d) => d,
                Err(_) => return Err(Error::InvalidConversation),
            };

        document.verify()?;

        if document.deleted {
            return Err(Error::InvalidConversation);
        }

        Ok(document)
    }

    pub async fn get_keystore(&self, id: Uuid) -> Result<Keystore, Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        self.root.get_conversation_keystore(id).await
    }

    pub async fn set_keystore(&mut self, id: Uuid, document: Keystore) -> Result<(), Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        let mut map = self.root.get_conversation_keystore_map().await?;

        let id = id.to_string();
        let cid = self.ipfs.dag().put().serialize(document)?.await?;

        map.insert(id, cid);

        self.set_keystore_map(map).await
    }

    pub async fn delete(&mut self, id: Uuid) -> Result<ConversationDocument, Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        let mut conversation = self.get(id).await?;

        if conversation.deleted {
            return Err(Error::InvalidConversation);
        }

        let list = conversation.messages.take();
        conversation.deleted = true;

        self.set_document(conversation.clone()).await?;

        if let Ok(mut ks_map) = self.root.get_conversation_keystore_map().await {
            if ks_map.remove(&id.to_string()).is_some() {
                if let Err(e) = self.set_keystore_map(ks_map).await {
                    warn!(conversation_id = %id, "Failed to remove keystore: {e}");
                }
            }
        }

        if let Some(cid) = list {
            let _ = self.ipfs.remove_block(cid, true).await;
        }

        Ok(conversation)
    }

    pub async fn list(&self) -> Vec<ConversationDocument> {
        self.list_stream().collect::<Vec<_>>().await
    }

    pub fn list_stream(&self) -> impl Stream<Item = ConversationDocument> + Unpin {
        let cid = match self.cid {
            Some(cid) => cid,
            None => return futures::stream::empty().boxed(),
        };

        let ipfs = self.ipfs.clone();

        let stream = async_stream::stream! {
            let conversation_map: BTreeMap<String, Cid> = ipfs
                .get_dag(cid)
                .local()
                .deserialized()
                .await
                .unwrap_or_default();

            let unordered = FuturesUnordered::from_iter(
                                conversation_map
                                    .values()
                                    .map(|cid| ipfs.get_dag(*cid).local().deserialized().into_future()),
                            )
                            .filter_map(|result: Result<ConversationDocument, _>| async move { result.ok() })
                            .filter(|document| {
                                let deleted = document.deleted;
                                async move { !deleted }
                            });

            for await conversation in unordered {
                yield conversation;
            }
        };

        stream.boxed()
    }

    pub async fn contains(&self, id: Uuid) -> bool {
        self.list_stream()
            .any(|conversation| async move { conversation.id() == id })
            .await
    }

    pub async fn set_keystore_map(&mut self, map: BTreeMap<String, Cid>) -> Result<(), Error> {
        self.root.set_conversation_keystore_map(map).await
    }

    pub async fn set_map(&mut self, map: BTreeMap<String, Cid>) -> Result<(), Error> {
        let cid = self.ipfs.dag().put().serialize(map)?.pin(true).await?;

        let old_map_cid = self.cid.replace(cid);

        if let Some(path) = self.path.as_ref() {
            let cid = cid.to_string();
            if let Err(e) = tokio::fs::write(path.join(".message_id"), cid).await {
                tracing::error!("Error writing to '.message_id': {e}.")
            }
        }

        if let Some(old_cid) = old_map_cid {
            if old_cid != cid && self.ipfs.is_pinned(&old_cid).await.unwrap_or_default() {
                self.ipfs.remove_pin(&old_cid).recursive().await?;
            }
        }

        Ok(())
    }

    pub async fn set_document(&mut self, mut document: ConversationDocument) -> Result<(), Error> {
        if let Some(creator) = document.creator.as_ref() {
            if creator.eq(&self.keypair)
                && matches!(document.conversation_type, ConversationType::Group { .. })
            {
                document.sign(&self.keypair)?;
            }
        }

        document.verify()?;

        let mut map = match self.cid {
            Some(cid) => self.ipfs.get_dag(cid).local().deserialized().await?,
            None => BTreeMap::new(),
        };

        let id = document.id().to_string();
        let cid = self.ipfs.dag().put().serialize(document)?.await?;

        map.insert(id, cid);

        self.set_map(map).await
    }

    pub async fn subscribe(
        &mut self,
        id: Uuid,
    ) -> Result<tokio::sync::broadcast::Sender<MessageEventKind>, Error> {
        if !self.contains(id).await {
            return Err(Error::InvalidConversation);
        }

        if let Some(tx) = self.event_handler.get(&id) {
            return Ok(tx.clone());
        }

        let (tx, _) = tokio::sync::broadcast::channel(1024);

        self.event_handler.insert(id, tx.clone());

        Ok(tx)
    }

    async fn process_msg_event(&mut self, id: Uuid, msg: Message) -> Result<(), Error> {
        let data = Payload::from_bytes(&msg.data)?;

        let own_did = &*self.keypair;

        let conversation = self.get(id).await.expect("Conversation exist");

        let bytes = match conversation.conversation_type {
            ConversationType::Direct => {
                let Some(recipient) = conversation
                    .recipients()
                    .iter()
                    .filter(|did| own_did.ne(did))
                    .cloned()
                    .collect::<Vec<_>>()
                    .first()
                    .cloned()
                else {
                    tracing::warn!(id = %id, "participant is not in conversation");
                    return Err(Error::IdentityDoesntExist);
                };
                ecdh_decrypt(own_did, Some(&recipient), data.data())?
            }
            ConversationType::Group { .. } => {
                let key = self.get_keystore(id).await.and_then(|store| store.get_latest(own_did, &data.sender())).map_err(|e| {
                    tracing::warn!(id = %id, sender = %data.sender(), error = %e, "Failed to obtain key");
                    e
                })?;

                Cipher::direct_decrypt(data.data(), &key)?
            }
        };

        let event = serde_json::from_slice::<MessagingEvents>(&bytes).map_err(|e| {
            tracing::warn!(id = %id, sender = %data.sender(), error = %e, "Failed to deserialize message");
            e
        })?;

        message_event(self, id, &event, MessageDirection::In, Default::default()).await?;

        Ok(())
    }

    //TODO: Use function to request key
    #[allow(dead_code)]
    async fn request_key(&mut self, conversation_id: Uuid, did: &DID) -> Result<(), Error> {
        let request = ConversationRequestResponse::Request {
            conversation_id,
            kind: ConversationRequestKind::Key,
        };

        let conversation = self.get(conversation_id).await?;

        if !conversation.recipients().contains(did) {
            //TODO: user is not a recipient of the conversation
            return Err(Error::PublicKeyInvalid);
        }

        let own_did = &self.keypair;

        let bytes = ecdh_encrypt(own_did, Some(did), serde_json::to_vec(&request)?)?;
        let signature = sign_serde(own_did, &bytes)?;

        let payload = Payload::new(own_did, &bytes, &signature);

        let topic = conversation.reqres_topic(did);

        let peers = self.ipfs.pubsub_peers(Some(topic.clone())).await?;
        let peer_id = did.to_peer_id()?;

        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(topic.clone(), payload.to_bytes()?.into())
                    .await
                    .is_err())
        {
            warn!("Unable to publish to topic");
            // if let Err(e) = self
            //     .queue_event(
            //         did.clone(),
            //         Queue::direct(
            //             conversation_id,
            //             None,
            //             peer_id,
            //             topic.clone(),
            //             payload.data().into(),
            //         ),
            //     )
            //     .await
            // {
            //     error!("Error submitting event to queue: {e}");
            // }
        }

        // TODO: Store request locally and hold any messages and events until key is received from peer

        Ok(())
    }
}

enum ConversationStreamData {
    RequestResponse(Message),
    Event(Message),
    Message(Message),
}

async fn process_conversation(
    this: &mut ConversationTask,
    data: Payload<'_>,
    event: ConversationEvents,
) -> anyhow::Result<()> {
    match event {
        ConversationEvents::NewConversation {
            recipient,
            settings,
        } => {
            let did = &*this.keypair;
            tracing::info!("New conversation event received from {recipient}");
            let conversation_id =
                generate_shared_topic(did, &recipient, Some("direct-conversation"))?;

            if this.contains(conversation_id).await {
                tracing::warn!("Conversation with {conversation_id} exist");
                return Ok(());
            }

            // if let Ok(true) = self.identity.is_blocked(&recipient).await {
            //     //TODO: Signal back to close conversation
            //     tracing::warn!("{recipient} is blocked");
            //     return Ok(());
            // }

            let list = [did.clone(), recipient];
            tracing::info!("Creating conversation");

            let convo = ConversationDocument::new_direct(did, list, settings)?;

            tracing::info!(
                "{} conversation created: {}",
                convo.conversation_type,
                conversation_id
            );

            let main_topic = convo.topic();
            let event_topic = convo.event_topic();
            let request_topic = convo.reqres_topic(&this.keypair);

            this.set_document(convo).await?;

            let messaging_stream = this
                .ipfs
                .pubsub_subscribe(main_topic)
                .await?
                .map(ConversationStreamData::Message)
                .boxed();

            let event_stream = this
                .ipfs
                .pubsub_subscribe(event_topic)
                .await?
                .map(ConversationStreamData::Event)
                .boxed();

            let request_stream = this
                .ipfs
                .pubsub_subscribe(request_topic)
                .await?
                .map(ConversationStreamData::RequestResponse)
                .boxed();

            let mut stream = messaging_stream
                .chain(event_stream)
                .chain(request_stream)
                .boxed();

            let (mut tx, rx) = mpsc::channel(256);

            tokio::spawn(async move {
                while let Some(stream_type) = stream.next().await {
                    if let Err(e) = tx.send(stream_type).await {
                        if e.is_disconnected() {
                            break;
                        }
                    }
                }
            });

            this.topic_stream.insert(conversation_id, rx);

            // self.start_task(this, stream).await;

            // self.event
            //     .emit(RayGunEventKind::ConversationCreated {
            //         conversation_id: id,
            //     })
            //     .await;
        }
        ConversationEvents::NewGroupConversation { mut conversation } => {
            let conversation_id = conversation.id;
            tracing::info!("New group conversation event received");

            if this.contains(conversation_id).await {
                warn!("Conversation with {conversation_id} exist");
                return Ok(());
            }

            if !conversation.recipients.contains(&this.keypair) {
                warn!(%conversation_id, "was added to conversation but never was apart of the conversation.");
                return Ok(());
            }

            // for recipient in conversation.recipients.iter() {
            //     if !self.discovery.contains(recipient).await {
            //         let _ = self.discovery.insert(recipient).await;
            //     }
            // }

            tracing::info!(%conversation_id, "Creating group conversation");

            let conversation_type = conversation.conversation_type;

            let mut keystore = Keystore::new(conversation_id);
            keystore.insert(&this.keypair, &this.keypair, warp::crypto::generate::<64>())?;

            conversation.verify()?;

            //TODO: Resolve message list
            conversation.messages = None;

            let main_topic = conversation.topic();
            let event_topic = conversation.event_topic();
            let request_topic = conversation.reqres_topic(&this.keypair);

            this.set_document(conversation).await?;

            tracing::info!(%conversation_id, "{} conversation created", conversation_type);

            this.set_keystore(conversation_id, keystore).await?;

            let messaging_stream = this
                .ipfs
                .pubsub_subscribe(main_topic)
                .await?
                .map(ConversationStreamData::Message)
                .boxed();

            let event_stream = this
                .ipfs
                .pubsub_subscribe(event_topic)
                .await?
                .map(ConversationStreamData::Event)
                .boxed();

            let request_stream = this
                .ipfs
                .pubsub_subscribe(request_topic)
                .await?
                .map(ConversationStreamData::RequestResponse)
                .boxed();

            let mut stream = messaging_stream
                .chain(event_stream)
                .chain(request_stream)
                .boxed();

            let (mut tx, rx) = mpsc::channel(256);

            tokio::spawn(async move {
                while let Some(stream_type) = stream.next().await {
                    if let Err(e) = tx.send(stream_type).await {
                        if e.is_disconnected() {
                            break;
                        }
                    }
                }
            });

            this.topic_stream.insert(conversation_id, rx);

            // self.start_task(conversation_id, stream).await;

            let conversation = this.get(conversation_id).await?;

            tracing::info!(%conversation_id,"{} conversation created", conversation_type);

            for _recipient in conversation
                .recipients
                .iter()
                .filter(|d| (*this.keypair).ne(d))
            {
                // if let Err(e) = self.request_key(conversation_id, recipient).await {
                //     tracing::warn!("Failed to send exchange request to {recipient}: {e}");
                // }
            }

            // self.event
            //     .emit(RayGunEventKind::ConversationCreated { conversation_id })
            //     .await;
        }
        ConversationEvents::LeaveConversation {
            conversation_id,
            recipient,
            signature,
        } => {
            let conversation = this.get(conversation_id).await?;

            if !matches!(
                conversation.conversation_type,
                ConversationType::Group { .. }
            ) {
                return Err(anyhow::anyhow!("Can only leave from a group conversation"));
            }

            let Some(creator) = conversation.creator.as_ref() else {
                return Err(anyhow::anyhow!("Group conversation requires a creator"));
            };

            let own_did = &*this.keypair;

            // Precaution
            if recipient.eq(creator) {
                return Err(anyhow::anyhow!("Cannot remove the creator of the group"));
            }

            if !conversation.recipients.contains(&recipient) {
                return Err(anyhow::anyhow!(
                    "{recipient} does not belong to {conversation_id}"
                ));
            }

            tracing::info!("{recipient} is leaving group conversation {conversation_id}");

            if creator.eq(own_did) {
                // self.remove_recipient(conversation_id, &recipient, false)
                //     .await?;
            } else {
                {
                    //Small validation context
                    let context = format!("exclude {}", recipient);
                    let signature = bs58::decode(&signature).into_vec()?;
                    verify_serde_sig(recipient.clone(), &context, &signature)?;
                }

                let mut conversation = this.get(conversation_id).await?;

                //Validate again since we have a permit
                if !conversation.recipients.contains(&recipient) {
                    return Err(anyhow::anyhow!(
                        "{recipient} does not belong to {conversation_id}"
                    ));
                }

                let mut can_emit = false;

                if let HashEntry::Vacant(entry) = conversation.excluded.entry(recipient.clone()) {
                    entry.insert(signature);
                    can_emit = true;
                }
                this.set_document(conversation).await?;
                if can_emit {
                    // let tx = self.get_conversation_sender(conversation_id).await?;
                    // if let Err(e) = tx.send(MessageEventKind::RecipientRemoved {
                    //     conversation_id,
                    //     recipient,
                    // }) {
                    //     tracing::error!("Error broadcasting event: {e}");
                    // }
                }
            }
        }
        ConversationEvents::DeleteConversation { conversation_id } => {
            tracing::trace!("Delete conversation event received for {conversation_id}");
            if !this.contains(conversation_id).await {
                anyhow::bail!("Conversation {conversation_id} doesnt exist");
            }

            let sender = data.sender();

            match this.get(conversation_id).await {
                Ok(conversation)
                    if conversation.recipients().contains(&sender)
                        && matches!(conversation.conversation_type, ConversationType::Direct)
                        || matches!(conversation.conversation_type, ConversationType::Group)
                            && matches!(&conversation.creator, Some(creator) if creator.eq(&sender)) =>
                {
                    conversation
                }
                _ => {
                    anyhow::bail!("Conversation exist but did not match condition required");
                }
            };

            // self.end_task(conversation_id).await;

            let _document = this.delete(conversation_id).await?;

            // let topic = document.topic();
            // self.queue.write().await.remove(&sender);

            // if this.ipfs.pubsub_unsubscribe(&topic).await.is_ok() {
            //     warn!(conversation_id = %document.id(), "topic should have been unsubscribed after dropping conversation.");
            // }

            // self.event
            //     .emit(RayGunEventKind::ConversationDeleted { conversation_id })
            //     .await;
        }
    }
    Ok(())
}

async fn message_event(
    this: &mut ConversationTask,
    conversation_id: Uuid,
    events: &MessagingEvents,
    direction: MessageDirection,
    _: EventOpt,
) -> Result<(), Error> {
    let mut document = this.get(conversation_id).await?;
    let tx = this.subscribe(conversation_id).await?;

    let keystore = match document.conversation_type {
        ConversationType::Direct => None,
        ConversationType::Group { .. } => this.get_keystore(conversation_id).await.ok(),
    };

    match events.clone() {
        MessagingEvents::New { message } => {
            let mut messages = document.get_message_list(&this.ipfs).await?;
            if messages
                .iter()
                .any(|message_document| message_document.id == message.id())
            {
                return Err(Error::MessageFound);
            }

            if !document.recipients().contains(&message.sender()) {
                return Err(Error::IdentityDoesntExist);
            }

            let lines_value_length: usize = message
                .lines()
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().count())
                .sum();

            if lines_value_length == 0 && lines_value_length > 4096 {
                tracing::error!(
                    message_length = lines_value_length,
                    "Length of message is invalid."
                );
                return Err(Error::InvalidLength {
                    context: "message".into(),
                    current: lines_value_length,
                    minimum: Some(1),
                    maximum: Some(4096),
                });
            }

            {
                let signature = message.signature();
                let sender = message.sender();
                let construct = [
                    message.id().into_bytes().to_vec(),
                    message.conversation_id().into_bytes().to_vec(),
                    sender.to_string().as_bytes().to_vec(),
                    message
                        .lines()
                        .iter()
                        .map(|s| s.as_bytes())
                        .collect::<Vec<_>>()
                        .concat(),
                ]
                .concat();
                verify_serde_sig(sender, &construct, &signature)?;
            }

            // spam_check(&mut message, self.spam_filter.clone())?;
            let conversation_id = message.conversation_id();

            let message_id = message.id();

            let message_document =
                MessageDocument::new(&this.ipfs, this.keypair.clone(), message, keystore.as_ref())
                    .await?;

            messages.insert(message_document);
            document.set_message_list(&this.ipfs, messages).await?;
            this.set_document(document).await?;

            let event = match direction {
                MessageDirection::In => MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                },
                MessageDirection::Out => MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                },
            };

            if let Err(e) = tx.send(event) {
                error!("Error broadcasting event: {e}");
            }
        }
        MessagingEvents::Edit {
            conversation_id,
            message_id,
            modified,
            lines,
            signature,
        } => {
            let mut list = document.get_message_list(&this.ipfs).await?;

            let mut message_document = list
                .iter()
                .find(|document| {
                    document.id == message_id && document.conversation_id == conversation_id
                })
                .copied()
                .ok_or(Error::MessageNotFound)?;

            let mut message = message_document
                .resolve(&this.ipfs, &this.keypair, keystore.as_ref())
                .await?;

            let lines_value_length: usize = lines
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().count())
                .sum();

            if lines_value_length == 0 && lines_value_length > 4096 {
                error!("Length of message is invalid: Got {lines_value_length}; Expected 4096");
                return Err(Error::InvalidLength {
                    context: "message".into(),
                    current: lines_value_length,
                    minimum: Some(1),
                    maximum: Some(4096),
                });
            }

            let sender = message.sender();
            //Validate the original message
            {
                let signature = message.signature();
                let construct = [
                    message.id().into_bytes().to_vec(),
                    message.conversation_id().into_bytes().to_vec(),
                    sender.to_string().as_bytes().to_vec(),
                    message
                        .lines()
                        .iter()
                        .map(|s| s.as_bytes())
                        .collect::<Vec<_>>()
                        .concat(),
                ]
                .concat();
                verify_serde_sig(sender.clone(), &construct, &signature)?;
            }

            //Validate the edit message
            {
                let construct = [
                    message.id().into_bytes().to_vec(),
                    message.conversation_id().into_bytes().to_vec(),
                    sender.to_string().as_bytes().to_vec(),
                    lines
                        .iter()
                        .map(|s| s.as_bytes())
                        .collect::<Vec<_>>()
                        .concat(),
                ]
                .concat();
                verify_serde_sig(sender, &construct, &signature)?;
            }

            message.set_signature(Some(signature));
            *message.lines_mut() = lines;
            message.set_modified(modified);

            message_document
                .update(&this.ipfs, &this.keypair, message, keystore.as_ref())
                .await?;
            list.replace(message_document);
            document.set_message_list(&this.ipfs, list).await?;
            this.set_document(document).await?;

            if let Err(e) = tx.send(MessageEventKind::MessageEdited {
                conversation_id,
                message_id,
            }) {
                error!("Error broadcasting event: {e}");
            }
        }
        MessagingEvents::Delete {
            conversation_id,
            message_id,
        } => {
            // if opt.keep_if_owned.load(Ordering::SeqCst) {
            //     let message_document = document
            //         .get_message_document(&self.ipfs, message_id)
            //         .await?;

            //     let message = message_document
            //         .resolve(&self.ipfs, &self.did, keystore.as_ref())
            //         .await?;

            //     let signature = message.signature();
            //     let sender = message.sender();
            //     let construct = [
            //         message.id().into_bytes().to_vec(),
            //         message.conversation_id().into_bytes().to_vec(),
            //         sender.to_string().as_bytes().to_vec(),
            //         message
            //             .lines()
            //             .iter()
            //             .map(|s| s.as_bytes())
            //             .collect::<Vec<_>>()
            //             .concat(),
            //     ]
            //     .concat();
            //     verify_serde_sig(sender, &construct, &signature)?;
            // }

            document.delete_message(&this.ipfs, message_id).await?;

            this.set_document(document).await?;

            if let Err(e) = tx.send(MessageEventKind::MessageDeleted {
                conversation_id,
                message_id,
            }) {
                error!("Error broadcasting event: {e}");
            }
        }
        MessagingEvents::Pin {
            conversation_id,
            message_id,
            state,
            ..
        } => {
            let mut list = document.get_message_list(&this.ipfs).await?;

            let mut message_document = document
                .get_message_document(&this.ipfs, message_id)
                .await?;

            let mut message = message_document
                .resolve(&this.ipfs, &this.keypair, keystore.as_ref())
                .await?;

            let event = match state {
                PinState::Pin => {
                    if message.pinned() {
                        return Ok(());
                    }
                    *message.pinned_mut() = true;
                    MessageEventKind::MessagePinned {
                        conversation_id,
                        message_id,
                    }
                }
                PinState::Unpin => {
                    if !message.pinned() {
                        return Ok(());
                    }
                    *message.pinned_mut() = false;
                    MessageEventKind::MessageUnpinned {
                        conversation_id,
                        message_id,
                    }
                }
            };

            message_document
                .update(&this.ipfs, &this.keypair, message, keystore.as_ref())
                .await?;

            list.replace(message_document);
            document.set_message_list(&this.ipfs, list).await?;
            this.set_document(document).await?;

            if let Err(e) = tx.send(event) {
                error!("Error broadcasting event: {e}");
            }
        }
        MessagingEvents::React {
            conversation_id,
            reactor,
            message_id,
            state,
            emoji,
        } => {
            let mut list = document.get_message_list(&this.ipfs).await?;

            let mut message_document = list
                .iter()
                .find(|document| {
                    document.id == message_id && document.conversation_id == conversation_id
                })
                .cloned()
                .ok_or(Error::MessageNotFound)?;

            let mut message = message_document
                .resolve(&this.ipfs, &this.keypair, keystore.as_ref())
                .await?;

            let reactions = message.reactions_mut();

            match state {
                ReactionState::Add => {
                    let entry = reactions.entry(emoji.clone()).or_default();

                    if entry.contains(&reactor) {
                        return Err(Error::ReactionExist);
                    }

                    entry.push(reactor.clone());

                    message_document
                        .update(&this.ipfs, &this.keypair, message, keystore.as_ref())
                        .await?;

                    list.replace(message_document);
                    document.set_message_list(&this.ipfs, list).await?;
                    this.set_document(document).await?;

                    if let Err(e) = tx.send(MessageEventKind::MessageReactionAdded {
                        conversation_id,
                        message_id,
                        did_key: reactor,
                        reaction: emoji,
                    }) {
                        error!("Error broadcasting event: {e}");
                    }
                }
                ReactionState::Remove => {
                    match reactions.entry(emoji.clone()) {
                        BTreeEntry::Occupied(mut e) => {
                            let list = e.get_mut();

                            if !list.contains(&reactor) {
                                return Err(Error::ReactionDoesntExist);
                            }

                            list.retain(|did| did != &reactor);
                            if list.is_empty() {
                                e.remove();
                            }
                        }
                        BTreeEntry::Vacant(_) => return Err(Error::ReactionDoesntExist),
                    };

                    message_document
                        .update(&this.ipfs, &this.keypair, message, keystore.as_ref())
                        .await?;

                    list.replace(message_document);
                    document.set_message_list(&this.ipfs, list).await?;

                    this.set_document(document).await?;

                    if let Err(e) = tx.send(MessageEventKind::MessageReactionRemoved {
                        conversation_id,
                        message_id,
                        did_key: reactor,
                        reaction: emoji,
                    }) {
                        error!("Error broadcasting event: {e}");
                    }
                }
            }
        }
        MessagingEvents::UpdateConversation {
            mut conversation,
            kind,
        } => {
            conversation.verify()?;
            match kind {
                ConversationUpdateKind::AddParticipant { did } => {
                    if document.recipients.contains(&did) {
                        return Err(Error::IdentityExist);
                    }

                    // if !self.discovery.contains(&did).await {
                    //     let _ = self.discovery.insert(&did).await.ok();
                    // }

                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    // if let Err(e) = self.request_key(conversation_id, &did).await {
                    //     error!("Error requesting key: {e}");
                    // }

                    if let Err(e) = tx.send(MessageEventKind::RecipientAdded {
                        conversation_id,
                        recipient: did,
                    }) {
                        error!("Error broadcasting event: {e}");
                    }
                }
                ConversationUpdateKind::RemoveParticipant { did } => {
                    if !document.recipients.contains(&did) {
                        return Err(Error::IdentityDoesntExist);
                    }

                    //Maybe remove participant from discovery?

                    let can_emit = !document.excluded.contains_key(&did);

                    document.excluded.remove(&did);

                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if can_emit {
                        if let Err(e) = tx.send(MessageEventKind::RecipientRemoved {
                            conversation_id,
                            recipient: did,
                        }) {
                            error!("Error broadcasting event: {e}");
                        }
                    }
                }
                ConversationUpdateKind::ChangeName { name: Some(name) } => {
                    let name = name.trim();
                    let name_length = name.len();

                    if name_length > 255 {
                        return Err(Error::InvalidLength {
                            context: "name".into(),
                            current: name_length,
                            minimum: None,
                            maximum: Some(255),
                        });
                    }
                    if let Some(current_name) = document.name() {
                        if current_name.eq(&name) {
                            return Ok(());
                        }
                    }

                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if let Err(e) = tx.send(MessageEventKind::ConversationNameUpdated {
                        conversation_id,
                        name: name.to_string(),
                    }) {
                        error!("Error broadcasting event: {e}");
                    }
                }

                ConversationUpdateKind::ChangeName { name: None } => {
                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if let Err(e) = tx.send(MessageEventKind::ConversationNameUpdated {
                        conversation_id,
                        name: String::new(),
                    }) {
                        error!("Error broadcasting event: {e}");
                    }
                }
                ConversationUpdateKind::AddRestricted { .. }
                | ConversationUpdateKind::RemoveRestricted { .. } => {
                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;
                    //TODO: Maybe add a api event to emit for when blocked users are added/removed from the document
                    //      but for now, we can leave this as a silent update since the block list would be for internal handling for now
                }
                ConversationUpdateKind::ChangeSettings { settings } => {
                    conversation.excluded = document.excluded;
                    conversation.messages = document.messages;
                    this.set_document(conversation).await?;

                    if let Err(e) = tx.send(MessageEventKind::ConversationSettingsUpdated {
                        conversation_id,
                        settings,
                    }) {
                        error!("Error broadcasting event: {e}");
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn process_request_response_event(
    this: &mut ConversationTask,
    conversation_id: Uuid,
    req: Message,
) -> Result<(), Error> {
    let conversation = this.get(conversation_id).await?;

    let payload = Payload::from_bytes(&req.data)?;

    let data = ecdh_decrypt(&this.keypair, Some(&payload.sender()), payload.data())?;

    let event = serde_json::from_slice::<ConversationRequestResponse>(&data)?;

    tracing::debug!(id = %conversation_id, ?event, "Event received");
    match event {
        ConversationRequestResponse::Request {
            conversation_id,
            kind,
        } => match kind {
            ConversationRequestKind::Key => {
                if !matches!(
                    conversation.conversation_type,
                    ConversationType::Group { .. }
                ) {
                    //Only group conversations support keys
                    unreachable!()
                }

                if !conversation.recipients().contains(&payload.sender()) {
                    warn!(
                        "{} is not apart of conversation {conversation_id}",
                        payload.sender()
                    );
                    return Err(Error::IdentityDoesntExist);
                }

                let mut keystore = this.get_keystore(conversation_id).await?;

                let raw_key = match keystore.get_latest(&this.keypair, &this.keypair) {
                    Ok(key) => key,
                    Err(Error::PublicKeyInvalid) => {
                        let key = generate::<64>().into();
                        keystore.insert(&this.keypair, &this.keypair, &key)?;

                        this.set_keystore(conversation_id, keystore).await?;
                        key
                    }
                    Err(e) => {
                        error!("Error getting key from store: {e}");
                        return Err(e);
                    }
                };
                let sender = payload.sender();
                let key = ecdh_encrypt(&this.keypair, Some(&sender), raw_key)?;

                let response = ConversationRequestResponse::Response {
                    conversation_id,
                    kind: ConversationResponseKind::Key { key },
                };

                let topic = conversation.reqres_topic(&sender);

                let bytes =
                    ecdh_encrypt(&this.keypair, Some(&sender), serde_json::to_vec(&response)?)?;
                let signature = sign_serde(&this.keypair, &bytes)?;

                let payload = Payload::new(&this.keypair, &bytes, &signature);

                let peers = this.ipfs.pubsub_peers(Some(topic.clone())).await?;

                let peer_id = sender.to_peer_id()?;

                let bytes = payload.to_bytes()?;

                tracing::trace!("Payload size: {} bytes", bytes.len());

                tracing::info!("Responding to {sender}");

                if !peers.contains(&peer_id)
                    || (peers.contains(&peer_id)
                        && this
                            .ipfs
                            .pubsub_publish(topic.clone(), bytes.into())
                            .await
                            .is_err())
                {
                    // warn!("Unable to publish to topic. Queuing event");
                    //     if let Err(e) = store
                    //         .queue_event(
                    //             sender.clone(),
                    //             Queue::direct(
                    //                 conversation_id,
                    //                 None,
                    //                 peer_id,
                    //                 topic.clone(),
                    //                 payload.data().into(),
                    //             ),
                    //         )
                    //         .await
                    //     {
                    //         error!("Error submitting event to queue: {e}");
                    //     }
                }
            }
            _ => {
                tracing::info!("Unimplemented/Unsupported Event");
            }
        },
        ConversationRequestResponse::Response {
            conversation_id,
            kind,
        } => match kind {
            ConversationResponseKind::Key { key } => {
                let sender = payload.sender();

                if !matches!(
                    conversation.conversation_type,
                    ConversationType::Group { .. }
                ) {
                    //Only group conversations support keys
                    tracing::error!(id = ?conversation_id, "Invalid conversation type");
                    unreachable!()
                }

                if !conversation.recipients().contains(&sender) {
                    return Err(Error::IdentityDoesntExist);
                }
                let mut keystore = this.get_keystore(conversation_id).await?;

                let raw_key = ecdh_decrypt(&this.keypair, Some(&sender), key)?;

                keystore.insert(&this.keypair, &sender, raw_key)?;

                this.set_keystore(conversation_id, keystore).await?;
            }
            _ => {
                tracing::info!("Unimplemented/Unsupported Event");
            }
        },
    }
    Ok(())
}

async fn process_conversation_event(
    this: &mut ConversationTask,
    conversation_id: Uuid,
    message: Message,
) -> Result<(), Error> {
    let tx = this.subscribe(conversation_id).await?;

    let conversation = this.get(conversation_id).await?;

    let payload = Payload::from_bytes(&message.data)?;

    let data = match conversation.conversation_type {
        ConversationType::Direct => {
            let recipient = conversation
                .recipients()
                .iter()
                .filter(|did| (*this.keypair).ne(did))
                .cloned()
                .collect::<Vec<_>>()
                .first()
                .cloned()
                .ok_or(Error::InvalidConversation)?;
            ecdh_decrypt(&this.keypair, Some(&recipient), payload.data())?
        }
        ConversationType::Group { .. } => {
            let keystore = this.get_keystore(conversation_id).await?;
            let key = keystore.get_latest(&this.keypair, &payload.sender())?;
            Cipher::direct_decrypt(payload.data(), &key)?
        }
    };

    let event = match serde_json::from_slice::<MessagingEvents>(&data)? {
        event @ MessagingEvents::Event { .. } => event,
        _ => return Err(Error::Other),
    };

    if let MessagingEvents::Event {
        conversation_id,
        member,
        event,
        cancelled,
    } = event
    {
        let ev = match cancelled {
            true => MessageEventKind::EventCancelled {
                conversation_id,
                did_key: member,
                event,
            },
            false => MessageEventKind::EventReceived {
                conversation_id,
                did_key: member,
                event,
            },
        };

        if let Err(e) = tx.send(ev) {
            tracing::error!("Error broadcasting event: {e}");
        }
    }

    Ok(())
}

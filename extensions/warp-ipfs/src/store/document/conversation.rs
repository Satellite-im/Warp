use std::{
    collections::{
        btree_map::Entry as BTreeEntry, hash_map::Entry as HashEntry, BTreeMap, HashMap, HashSet,
    },
    future::IntoFuture,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Instant,
};

use chrono::Utc;
use futures::{
    channel::{mpsc, oneshot},
    pin_mut,
    stream::{self, BoxStream, FuturesUnordered},
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
    constellation::{ConstellationProgressStream, Progression},
    crypto::{cipher::Cipher, generate, DID},
    error::Error,
    multipass::MultiPassEventKind,
    raygun::{
        AttachmentEventStream, AttachmentKind, Conversation, ConversationSettings,
        ConversationType, DirectConversationSettings, GroupSettings, Location, MessageEvent,
        MessageEventKind, MessageType, PinState, RayGunEventKind, ReactionState,
    },
};

use crate::store::{
    conversation::{ConversationDocument, MessageDocument},
    ecdh_decrypt, ecdh_encrypt,
    event_subscription::EventSubscription,
    files::FileStore,
    generate_shared_topic,
    keystore::Keystore,
    message::{EventOpt, MessageDirection},
    payload::Payload,
    sign_serde, verify_serde_sig, ConversationEvents, ConversationRequestKind,
    ConversationRequestResponse, ConversationResponseKind, ConversationUpdateKind, DidExt,
    MessagingEvents, PeerTopic,
};

use super::root::RootDocumentMap;

pub type DownloadStream = BoxStream<'static, Result<Vec<u8>, Error>>;

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
    LoadConversations {
        response: oneshot::Sender<Result<(), Error>>,
    },
    Subscribe {
        id: Uuid,
        response: oneshot::Sender<Result<tokio::sync::broadcast::Sender<MessageEventKind>, Error>>,
    },

    CreateConversation {
        did: DID,
        response: oneshot::Sender<Result<Conversation, Error>>,
    },
    CreateGroupConversation {
        name: Option<String>,
        recipients: HashSet<DID>,
        settings: GroupSettings,
        response: oneshot::Sender<Result<Conversation, Error>>,
    },
    DeleteConversation {
        conversation_id: Uuid,
        response: oneshot::Sender<Result<(), Error>>,
    },
    UpdateConversationName {
        conversation_id: Uuid,
        name: String,
        response: oneshot::Sender<Result<(), Error>>,
    },
    UpdateConversationSettings {
        conversation_id: Uuid,
        settings: ConversationSettings,
        response: oneshot::Sender<Result<(), Error>>,
    },
    AddRecipient {
        conversation_id: Uuid,
        did: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    RemoveRecipient {
        conversation_id: Uuid,
        did: DID,
        response: oneshot::Sender<Result<(), Error>>,
    },
    SendMessage {
        conversation_id: Uuid,
        lines: Vec<String>,
        response: oneshot::Sender<Result<(), Error>>,
    },
    EditMessage {
        conversation_id: Uuid,
        message_id: Uuid,
        lines: Vec<String>,
        response: oneshot::Sender<Result<(), Error>>,
    },
    Reply {
        conversation_id: Uuid,
        message_id: Uuid,
        lines: Vec<String>,
        response: oneshot::Sender<Result<(), Error>>,
    },
    DeleteMessage {
        conversation_id: Uuid,
        message_id: Uuid,
        response: oneshot::Sender<Result<(), Error>>,
    },
    PinMessage {
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
        response: oneshot::Sender<Result<(), Error>>,
    },
    React {
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
        response: oneshot::Sender<Result<(), Error>>,
    },
    Attach {
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        locations: Vec<Location>,
        messages: Vec<String>,
        response: oneshot::Sender<Result<AttachmentEventStream, Error>>,
    },
    Download {
        conversation_id: Uuid,
        message_id: Uuid,
        file: String,
        path: PathBuf,
        response: oneshot::Sender<Result<ConstellationProgressStream, Error>>,
    },
    DownloadStream {
        conversation_id: Uuid,
        message_id: Uuid,
        file: String,
        response: oneshot::Sender<Result<DownloadStream, Error>>,
    },
    SendEvent {
        conversation_id: Uuid,
        event: MessageEvent,
        response: oneshot::Sender<Result<(), Error>>,
    },
    CancelEvent {
        conversation_id: Uuid,
        event: MessageEvent,
        response: oneshot::Sender<Result<(), Error>>,
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
        file: FileStore,
        event: EventSubscription<RayGunEventKind>,
        identity_event: BoxStream<'static, MultiPassEventKind>,
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
            let mut task = ConversationTask {
                ipfs,
                event_handler: Default::default(),
                keypair,
                path,
                cid,
                topic_stream: StreamMap::new(),
                rx,
                root,
                file,
                event,
            };
            select! {
                _ = token.cancelled() => {}
                _ = task.run(identity_event) => {}
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

    pub async fn load_conversations(&self) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::LoadConversations { response: tx })
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

    pub async fn create_conversation(&self, did: &DID) -> Result<Conversation, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::CreateConversation {
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn create_group_conversation(
        &self,
        name: Option<String>,
        members: HashSet<DID>,
        settings: GroupSettings,
    ) -> Result<Conversation, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::CreateGroupConversation {
                name,
                recipients: members,
                settings,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn update_conversation_name<S: Into<String>>(
        &self,
        conversation_id: Uuid,
        name: S,
    ) -> Result<(), Error> {
        let name = name.into();
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::UpdateConversationName {
                conversation_id,
                name,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn update_conversation_settings(
        &self,
        conversation_id: Uuid,
        settings: ConversationSettings,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::UpdateConversationSettings {
                conversation_id,
                settings,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn delete_conversation(&self, conversation_id: Uuid) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::DeleteConversation {
                conversation_id,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn add_recipient(&self, conversation_id: Uuid, did: &DID) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::AddRecipient {
                conversation_id,
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn remove_recipient(&self, conversation_id: Uuid, did: &DID) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::RemoveRecipient {
                conversation_id,
                did: did.clone(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn send_message(
        &self,
        conversation_id: Uuid,
        lines: Vec<String>,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::SendMessage {
                conversation_id,
                lines,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn edit_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        lines: Vec<String>,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::EditMessage {
                conversation_id,
                message_id,
                lines,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn reply(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        lines: Vec<String>,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::Reply {
                conversation_id,
                message_id,
                lines,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn delete_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::DeleteMessage {
                conversation_id,
                message_id,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn pin_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::PinMessage {
                conversation_id,
                message_id,
                state,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn react<S: Into<String>>(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: S,
    ) -> Result<(), Error> {
        let emoji = emoji.into();
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::React {
                conversation_id,
                message_id,
                state,
                emoji,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn attach(
        &self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        locations: Vec<Location>,
        messages: Vec<String>,
    ) -> Result<AttachmentEventStream, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::Attach {
                conversation_id,
                message_id,
                locations,
                messages,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn download<S: Into<String>, P: AsRef<Path>>(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: S,
        path: P,
    ) -> Result<ConstellationProgressStream, Error> {
        let file = file.into();
        let path = path.as_ref().to_path_buf();

        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::Download {
                conversation_id,
                message_id,
                file,
                path,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn download_stream<S: Into<String>>(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        file: S,
    ) -> Result<DownloadStream, Error> {
        let file = file.into();

        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::DownloadStream {
                conversation_id,
                message_id,
                file,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn send_event(
        &self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::SendEvent {
                conversation_id,
                event,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn cancel_event(
        &self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(ConversationCommand::CancelEvent {
                conversation_id,
                event,
                response: tx,
            })
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
    file: FileStore,
    event: EventSubscription<RayGunEventKind>,
    rx: mpsc::Receiver<ConversationCommand>,
}

impl ConversationTask {
    async fn run(&mut self, mut identity_event: BoxStream<'static, MultiPassEventKind>) {
        let stream = self
            .ipfs
            .pubsub_subscribe(self.keypair.messaging())
            .await
            .expect("valid subscription");

        pin_mut!(stream);

        loop {
            tokio::select! {
                biased;
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
                        ConversationCommand::LoadConversations { response } => {
                            let _ = response.send(self.load_conversations().await);
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
                        ConversationCommand::CreateConversation { did, response } => {
                            _ = response.send(self.create_conversation(&did).await);
                        },
                        ConversationCommand::CreateGroupConversation { name, recipients, settings, response } => {
                            _ = response.send(self.create_group_conversation(name, recipients, settings).await);
                        },
                        ConversationCommand::DeleteConversation { conversation_id, response } => {
                            _ = response.send(self.delete_conversation(conversation_id, true).await)
                        },
                        ConversationCommand::UpdateConversationName { conversation_id, name, response } => {
                            _ = response.send(self.update_conversation_name(conversation_id, &name).await)
                        },
                        ConversationCommand::UpdateConversationSettings { conversation_id, settings, response } => {
                            _ = response.send(self.update_conversation_settings(conversation_id, settings).await)
                        },
                        ConversationCommand::AddRecipient { conversation_id, did, response } => {
                            _ = response.send(self.add_recipient(conversation_id, &did).await)
                        },
                        ConversationCommand::RemoveRecipient { conversation_id, did, response } => {
                            _ = response.send(self.remove_recipient(conversation_id, &did, true).await)
                        },
                        ConversationCommand::SendMessage { conversation_id, lines, response } => {
                            _ = response.send(self.send_message(conversation_id, lines).await)
                        },
                        ConversationCommand::EditMessage { conversation_id, message_id, lines, response } => {
                            _ = response.send(self.edit_message(conversation_id, message_id, lines).await)
                        },
                        ConversationCommand::Reply { conversation_id, message_id, lines, response } => {
                            _ = response.send(self.reply_message(conversation_id, message_id, lines).await)
                        },
                        ConversationCommand::DeleteMessage { conversation_id, message_id, response } => {
                            _ = response.send(self.delete_message(conversation_id, message_id, true).await)
                        },
                        ConversationCommand::PinMessage { conversation_id, message_id, state, response } => {
                            _ = response.send(self.pin_message(conversation_id, message_id, state).await)
                        },
                        ConversationCommand::React { conversation_id, message_id, state, emoji, response } => {
                            _ = response.send(self.react(conversation_id, message_id, state, emoji).await)
                        },
                        ConversationCommand::Attach { conversation_id, message_id, locations, messages, response } => {
                            _ = response.send(self.attach(conversation_id, message_id, locations, messages).await)
                        },
                        ConversationCommand::Download { conversation_id, message_id, file, path, response } => {
                            _ = response.send(self.download(conversation_id, message_id, &file, path).await)
                        },
                        ConversationCommand::DownloadStream { conversation_id, message_id, file, response } => {
                            _ = response.send(self.download_stream(conversation_id, message_id, &file).await)
                        },
                        ConversationCommand::SendEvent { conversation_id, event, response } => {
                            _ = response.send(self.send_event(conversation_id, event).await)
                        }
                        ConversationCommand::CancelEvent { conversation_id, event, response } => {
                            _ = response.send(self.cancel_event(conversation_id, event).await)
                        }
                    }
                }
                Some(ev) = identity_event.next() => {
                    if let Err(e) = process_identity_events(self, ev).await {
                        tracing::error!("Error processing identity events: {e}");
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

    async fn load_conversations(&mut self) -> Result<(), Error> {
        let mut stream = self.list_stream();
        while let Some(conversation) = stream.next().await {
            let id = conversation.id();
            let loaded = async {
                let main_topic = conversation.topic();
                let event_topic = conversation.event_topic();
                let request_topic = conversation.reqres_topic(&self.keypair);

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

                self.topic_stream.insert(id, rx);
                Ok::<_, Error>(())
            };

            if let Err(e) = loaded.await {
                tracing::error!(id = %id, error = %e, "Failed to load conversation");
            }
        }
        Ok(())
    }

    async fn create_conversation(&mut self, did: &DID) -> Result<Conversation, Error> {
        //TODO: maybe use root document to directly check
        // if self.with_friends.load(Ordering::SeqCst) && !self.identity.is_friend(did_key).await? {
        //     return Err(Error::FriendDoesntExist);
        // }

        if self.root.is_blocked(did).await.unwrap_or_default() {
            return Err(Error::PublicKeyIsBlocked);
        }

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
            .map(Conversation::from)
        {
            return Err(Error::ConversationExist { conversation });
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

        self.event
            .emit(RayGunEventKind::ConversationCreated {
                conversation_id: convo_id,
            })
            .await;

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

        for did in recipients.iter() {
            let is_blocked = self.root.is_blocked(did).await?;
            let is_blocked_by = self.root.is_blocked_by(did).await?;
            if is_blocked || is_blocked_by {
                tracing::info!("{did} is blocked.. removing from list");
                removal.push(did.clone());
            }
        }

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
        let main_topic = conversation.topic();
        let event_topic = conversation.event_topic();
        let request_topic = conversation.reqres_topic(&self.keypair);

        self.set_document(conversation).await?;

        let mut keystore = Keystore::new(convo_id);
        keystore.insert(own_did, own_did, warp::crypto::generate::<64>())?;

        self.set_keystore(convo_id, keystore).await?;

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

        self.event
            .emit(RayGunEventKind::ConversationCreated {
                conversation_id: conversation.id(),
            })
            .await;

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

    async fn get_message(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<warp::raygun::Message, Error> {
        let conversation = self.get(conversation_id).await?;
        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => self.get_keystore(conversation.id()).await.ok(),
        };
        conversation
            .get_message(&self.ipfs, &self.keypair, message_id, keystore.as_ref())
            .await
    }
}

impl ConversationTask {
    pub async fn send_message(
        &mut self,
        conversation_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > 4096 {
            error!("Length of message is invalid: Got {lines_value_length}; Expected 4096");
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(1),
                maximum: Some(4096),
            });
        }

        let own_did = &*self.keypair;

        let mut message = warp::raygun::Message::default();
        message.set_conversation_id(conversation.id());
        message.set_sender(own_did.clone());
        message.set_lines(messages.clone());

        let construct = [
            message.id().into_bytes().to_vec(),
            message.conversation_id().into_bytes().to_vec(),
            own_did.to_string().as_bytes().to_vec(),
            message
                .lines()
                .iter()
                .map(|s| s.as_bytes())
                .collect::<Vec<_>>()
                .concat(),
        ]
        .concat();

        let signature = sign_serde(own_did, &construct)?;
        message.set_signature(Some(signature));

        let message_id = message.id();

        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => self.get_keystore(conversation_id).await.ok(),
        };

        let message_document = MessageDocument::new(
            &self.ipfs,
            self.keypair.clone(),
            message.clone(),
            keystore.as_ref(),
        )
        .await?;

        let mut messages = conversation.get_message_list(&self.ipfs).await?;
        messages.insert(message_document);
        conversation.set_message_list(&self.ipfs, messages).await?;
        self.set_document(conversation).await?;

        let event = MessageEventKind::MessageSent {
            conversation_id,
            message_id,
        };

        if let Err(e) = tx.send(event) {
            error!("Error broadcasting event: {e}");
        }

        let event = MessagingEvents::New { message };

        self.publish(conversation_id, Some(message_id), event, true)
            .await
    }

    pub async fn edit_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;
        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > 4096 {
            error!("Length of message is invalid: Got {lines_value_length}; Expected 4096");
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(1),
                maximum: Some(4096),
            });
        }

        let mut message = self.get_message(conversation_id, message_id).await?;

        let sender = message.sender();

        let own_did = &*self.keypair;

        if sender.ne(own_did) {
            return Err(Error::InvalidMessage);
        }

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

        let construct = [
            message_id.into_bytes().to_vec(),
            conversation.id().into_bytes().to_vec(),
            own_did.to_string().as_bytes().to_vec(),
            messages
                .iter()
                .map(|s| s.as_bytes())
                .collect::<Vec<_>>()
                .concat(),
        ]
        .concat();

        let signature = sign_serde(own_did, &construct)?;

        let modified = Utc::now();

        message.set_signature(Some(signature.clone()));
        *message.lines_mut() = messages.clone();
        message.set_modified(modified);

        let event = MessagingEvents::Edit {
            conversation_id: conversation.id(),
            message_id,
            modified,
            lines: messages,
            signature,
        };

        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => self.get_keystore(conversation_id).await.ok(),
        };

        let mut list = conversation.get_message_list(&self.ipfs).await?;

        //TODO: Maybe assert?
        let mut message_document = list
            .iter()
            .find(|document| {
                document.id == message_id && document.conversation_id == conversation_id
            })
            .copied()
            .ok_or(Error::MessageNotFound)?;

        message_document
            .update(&self.ipfs, &self.keypair, message, keystore.as_ref())
            .await?;

        list.replace(message_document);
        conversation.set_message_list(&self.ipfs, list).await?;
        self.set_document(conversation).await?;

        _ = tx.send(MessageEventKind::MessageEdited {
            conversation_id,
            message_id,
        });

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn reply_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        messages: Vec<String>,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        if messages.is_empty() {
            return Err(Error::EmptyMessage);
        }

        let lines_value_length: usize = messages
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length == 0 || lines_value_length > 4096 {
            error!("Length of message is invalid: Got {lines_value_length}; Expected 4096");
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: Some(1),
                maximum: Some(4096),
            });
        }

        let own_did = &*self.keypair;

        let mut message = warp::raygun::Message::default();
        message.set_conversation_id(conversation.id());
        message.set_sender(own_did.clone());
        message.set_lines(messages);
        message.set_replied(Some(message_id));

        let construct = [
            message.id().into_bytes().to_vec(),
            message.conversation_id().into_bytes().to_vec(),
            own_did.to_string().as_bytes().to_vec(),
            message
                .lines()
                .iter()
                .map(|s| s.as_bytes())
                .collect::<Vec<_>>()
                .concat(),
        ]
        .concat();

        let signature = sign_serde(own_did, &construct)?;
        message.set_signature(Some(signature));

        let message_id = message.id();

        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => self.get_keystore(conversation_id).await.ok(),
        };

        let message_document = MessageDocument::new(
            &self.ipfs,
            self.keypair.clone(),
            message.clone(),
            keystore.as_ref(),
        )
        .await?;

        let mut messages = conversation.get_message_list(&self.ipfs).await?;
        messages.insert(message_document);
        conversation.set_message_list(&self.ipfs, messages).await?;
        self.set_document(conversation).await?;

        let event = MessageEventKind::MessageSent {
            conversation_id,
            message_id,
        };

        if let Err(e) = tx.send(event) {
            error!("Error broadcasting event: {e}");
        }

        let event = MessagingEvents::New { message };

        self.publish(conversation_id, Some(message_id), event, true)
            .await
    }

    pub async fn delete_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        broadcast: bool,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        let event = MessagingEvents::Delete {
            conversation_id: conversation.id(),
            message_id,
        };

        conversation.delete_message(&self.ipfs, message_id).await?;

        self.set_document(conversation).await?;

        _ = tx.send(MessageEventKind::MessageDeleted {
            conversation_id,
            message_id,
        });

        if broadcast {
            self.publish(conversation_id, None, event, true).await?;
        }

        Ok(())
    }

    pub async fn pin_message(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => self.get_keystore(conversation_id).await.ok(),
        };

        let mut list = conversation.get_message_list(&self.ipfs).await?;

        let mut message_document = conversation
            .get_message_document(&self.ipfs, message_id)
            .await?;

        let mut message = message_document
            .resolve(&self.ipfs, &self.keypair, keystore.as_ref())
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
            .update(&self.ipfs, &self.keypair, message, keystore.as_ref())
            .await?;

        list.replace(message_document);
        conversation.set_message_list(&self.ipfs, list).await?;
        self.set_document(conversation).await?;

        _ = tx.send(event);

        let event = MessagingEvents::Pin {
            conversation_id,
            member: (*self.keypair).clone(),
            message_id,
            state,
        };

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

        let keystore = match conversation.conversation_type {
            ConversationType::Direct => None,
            ConversationType::Group { .. } => self.get_keystore(conversation_id).await.ok(),
        };

        let mut list = conversation.get_message_list(&self.ipfs).await?;

        let mut message_document = conversation
            .get_message_document(&self.ipfs, message_id)
            .await?;

        let mut message = message_document
            .resolve(&self.ipfs, &self.keypair, keystore.as_ref())
            .await?;

        let reactions = message.reactions_mut();

        match state {
            ReactionState::Add => {
                let entry = reactions.entry(emoji.clone()).or_default();

                if entry.contains(&self.keypair) {
                    return Err(Error::ReactionExist);
                }

                entry.push((*self.keypair).clone());

                message_document
                    .update(&self.ipfs, &self.keypair, message, keystore.as_ref())
                    .await?;

                list.replace(message_document);
                conversation.set_message_list(&self.ipfs, list).await?;
                self.set_document(conversation).await?;

                _ = tx.send(MessageEventKind::MessageReactionAdded {
                    conversation_id,
                    message_id,
                    did_key: (*self.keypair).clone(),
                    reaction: emoji.clone(),
                });
            }
            ReactionState::Remove => {
                match reactions.entry(emoji.clone()) {
                    BTreeEntry::Occupied(mut e) => {
                        let list = e.get_mut();

                        if !list.contains(&self.keypair) {
                            return Err(Error::ReactionDoesntExist);
                        }

                        list.retain(|did| did != &(*self.keypair).clone());
                        if list.is_empty() {
                            e.remove();
                        }
                    }
                    BTreeEntry::Vacant(_) => return Err(Error::ReactionDoesntExist),
                };

                message_document
                    .update(&self.ipfs, &self.keypair, message, keystore.as_ref())
                    .await?;

                list.replace(message_document);
                conversation.set_message_list(&self.ipfs, list).await?;

                self.set_document(conversation).await?;

                _ = tx.send(MessageEventKind::MessageReactionRemoved {
                    conversation_id,
                    message_id,
                    did_key: (*self.keypair).clone(),
                    reaction: emoji.clone(),
                });
            }
        }

        let event = MessagingEvents::React {
            conversation_id,
            reactor: (*self.keypair).clone(),
            message_id,
            state,
            emoji,
        };

        // let (one_tx, one_rx) = oneshot::channel();
        // tx.send((event.clone(), Some(one_tx)))
        //     .await
        //     .map_err(anyhow::Error::from)?;
        // one_rx.await.map_err(anyhow::Error::from)??;

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn attach(
        &mut self,
        _conversation_id: Uuid,
        _message_id: Option<Uuid>,
        _locations: Vec<Location>,
        _messages: Vec<String>,
    ) -> Result<BoxStream<'static, AttachmentKind>, Error> {
        // if locations.len() > 8 {
        //     return Err(Error::InvalidLength {
        //         context: "files".into(),
        //         current: locations.len(),
        //         minimum: Some(1),
        //         maximum: Some(8),
        //     });
        // }

        // if !messages.is_empty() {
        //     let lines_value_length: usize = messages
        //         .iter()
        //         .filter(|s| !s.is_empty())
        //         .map(|s| s.trim())
        //         .map(|s| s.chars().count())
        //         .sum();

        //     if lines_value_length > 4096 {
        //         error!("Length of message is invalid: Got {lines_value_length}; Expected 4096");
        //         return Err(Error::InvalidLength {
        //             context: "message".into(),
        //             current: lines_value_length,
        //             minimum: None,
        //             maximum: Some(4096),
        //         });
        //     }
        // }
        // let conversation = self.get(conversation_id).await?;
        // let mut tx = self.subscribe(conversation_id).await?;

        // let mut constellation = self.file.clone();

        // let files = locations
        //     .iter()
        //     .filter(|location| match location {
        //         Location::Disk { path } => path.is_file(),
        //         _ => true,
        //     })
        //     .cloned()
        //     .collect::<Vec<_>>();

        // if files.is_empty() {
        //     return Err(Error::NoAttachments);
        // }

        // let stream = async_stream::stream! {
        //     let mut in_stack = vec![];

        //     let mut attachments = vec![];
        //     let mut total_thumbnail_size = 0;

        //     let mut streams: SelectAll<_> = SelectAll::new();

        //     for file in files {
        //         match file {
        //             Location::Constellation { path } => {
        //                 match constellation
        //                     .root_directory()
        //                     .get_item_by_path(&path)
        //                     .and_then(|item| item.get_file())
        //                 {
        //                     Ok(f) => {
        //                         let stream = async_stream::stream! {
        //                             yield (Progression::ProgressComplete { name: f.name(), total: Some(f.size()) }, Some(f));
        //                         };
        //                         streams.push(stream.boxed());
        //                     },
        //                     Err(e) => {
        //                         let constellation_path = PathBuf::from(&path);
        //                         let name = constellation_path.file_name().and_then(OsStr::to_str).map(str::to_string).unwrap_or(path);
        //                         let stream = async_stream::stream! {
        //                             yield (Progression::ProgressFailed { name, last_size: None, error: Some(e.to_string()) }, None);
        //                         };
        //                         streams.push(stream.boxed());
        //                     },
        //                 }
        //             }
        //             Location::Disk {path} => {
        //                 let mut filename = match path.file_name() {
        //                     Some(file) => file.to_string_lossy().to_string(),
        //                     None => continue,
        //                 };

        //                 let original = filename.clone();

        //                 let current_directory = match constellation.current_directory() {
        //                     Ok(directory) => directory,
        //                     Err(e) => {
        //                         yield AttachmentKind::Pending(Err(e));
        //                         return;
        //                     }
        //                 };

        //                 let mut interval = 0;
        //                 let skip;
        //                 loop {
        //                     if in_stack.contains(&filename) || current_directory.has_item(&filename) {
        //                         if interval > 2000 {
        //                             skip = true;
        //                             break;
        //                         }
        //                         interval += 1;
        //                         let file = PathBuf::from(&original);
        //                         let file_stem =
        //                             file.file_stem().and_then(OsStr::to_str).map(str::to_string);
        //                         let ext = file.extension().and_then(OsStr::to_str).map(str::to_string);

        //                         filename = match (file_stem, ext) {
        //                             (Some(filename), Some(ext)) => {
        //                                 format!("{filename} ({interval}).{ext}")
        //                             }
        //                             _ => format!("{original} ({interval})"),
        //                         };
        //                         continue;
        //                     }
        //                     skip = false;
        //                     break;
        //                 }

        //                 if skip {
        //                     let stream = async_stream::stream! {
        //                         yield (Progression::ProgressFailed { name: filename, last_size: None, error: Some("Max files reached".into()) }, None);
        //                     };
        //                     streams.push(stream.boxed());
        //                     continue;
        //                 }

        //                 let file = path.display().to_string();

        //                 in_stack.push(filename.clone());

        //                 let mut progress = match constellation.put(&filename, &file).await {
        //                     Ok(stream) => stream,
        //                     Err(e) => {
        //                         error!("Error uploading {filename}: {e}");
        //                         let stream = async_stream::stream! {
        //                             yield (Progression::ProgressFailed { name: filename, last_size: None, error: Some(e.to_string()) }, None);
        //                         };
        //                         streams.push(stream.boxed());
        //                         continue;
        //                     }
        //                 };

        //                 let current_directory = current_directory.clone();
        //                 let filename = filename.to_string();

        //                 let stream = async_stream::stream! {
        //                     while let Some(item) = progress.next().await {
        //                         match item {
        //                             item @ Progression::CurrentProgress { .. } => {
        //                                 yield (item, None);
        //                             },
        //                             item @ Progression::ProgressComplete { .. } => {
        //                                 let file = current_directory.get_item(&filename).and_then(|item| item.get_file()).ok();
        //                                 yield (item, file);
        //                                 break;
        //                             },
        //                             item @ Progression::ProgressFailed { .. } => {
        //                                 yield (item, None);
        //                                 break;
        //                             }
        //                         }
        //                     }
        //                 };

        //                 streams.push(stream.boxed());
        //             }
        //         };
        //     }

        //     for await (progress, file) in streams {
        //         yield AttachmentKind::AttachedProgress(progress);
        //         if let Some(file) = file {
        //             // We reconstruct it to avoid out any metadata that was apart of the `File` structure that we dont want to share
        //             let new_file = warp::constellation::file::File::new(&file.name());

        //             let thumbnail = file.thumbnail();

        //             if total_thumbnail_size < 3 * 1024 * 1024
        //                 && !thumbnail.is_empty()
        //                 && thumbnail.len() <= 1024 * 1024
        //             {
        //                 new_file.set_thumbnail(&thumbnail);
        //                 new_file.set_thumbnail_format(file.thumbnail_format());
        //                 total_thumbnail_size += thumbnail.len();
        //             }
        //             new_file.set_size(file.size());
        //             new_file.set_hash(file.hash());
        //             new_file.set_reference(&file.reference().unwrap_or_default());
        //             attachments.push(new_file);
        //         }
        //     }

        //     let final_results = {
        //         async move {

        //             if attachments.is_empty() {
        //                 return Err(Error::NoAttachments);
        //             }

        //             let own_did = &*self.keypair;
        //             let mut message = warp::raygun::Message::default();
        //             message.set_message_type(MessageType::Attachment);
        //             message.set_conversation_id(conversation.id());
        //             message.set_sender(own_did.clone());
        //             message.set_attachment(attachments);
        //             message.set_lines(messages.clone());
        //             message.set_replied(message_id);
        //             let construct = [
        //                 message.id().into_bytes().to_vec(),
        //                 message.conversation_id().into_bytes().to_vec(),
        //                 own_did.to_string().as_bytes().to_vec(),
        //                 message
        //                     .lines()
        //                     .iter()
        //                     .map(|s| s.as_bytes())
        //                     .collect::<Vec<_>>()
        //                     .concat(),
        //             ]
        //             .concat();

        //             let signature = sign_serde(own_did, &construct)?;
        //             message.set_signature(Some(signature));

        //             let event = MessagingEvents::New { message };
        //             // let (one_tx, one_rx) = oneshot::channel();
        //             // tx.send((event.clone(), Some(one_tx)))
        //             //     .await
        //             //     .map_err(anyhow::Error::from)?;
        //             // one_rx.await.map_err(anyhow::Error::from)??;
        //             self.publish(conversation_id, None, event, true).await
        //         }
        //     };

        //     yield AttachmentKind::Pending(final_results.await)
        // };

        //TODO
        Ok(stream::empty().boxed())
    }

    pub async fn download(
        &self,
        conversation: Uuid,
        message_id: Uuid,
        file: &str,
        path: PathBuf,
    ) -> Result<ConstellationProgressStream, Error> {
        let constellation = self.file.clone();

        let members = self.get(conversation).await.map(|c| {
            c.recipients()
                .iter()
                .filter_map(|did| did.to_peer_id().ok())
                .collect::<Vec<_>>()
        })?;

        let message = self.get_message(conversation, message_id).await?;

        if message.message_type() != MessageType::Attachment {
            return Err(Error::InvalidMessage);
        }

        let attachment = message
            .attachments()
            .iter()
            .find(|attachment| attachment.name() == file)
            .cloned()
            .ok_or(Error::FileNotFound)?;

        let _root = constellation.root_directory();

        let reference = attachment
            .reference()
            .and_then(|reference| IpfsPath::from_str(&reference).ok())
            .ok_or(Error::FileNotFound)?;

        let ipfs = self.ipfs.clone();
        let _constellation = constellation.clone();
        let progress_stream = async_stream::stream! {
            yield Progression::CurrentProgress {
                name: attachment.name(),
                current: 0,
                total: Some(attachment.size()),
            };

            let stream = ipfs.unixfs().get(reference, &path, &members, false, None);

            for await event in stream {
                match event {
                    rust_ipfs::unixfs::UnixfsStatus::ProgressStatus { written, total_size } => {
                        yield Progression::CurrentProgress {
                            name: attachment.name(),
                            current: written,
                            total: total_size
                        };
                    },
                    rust_ipfs::unixfs::UnixfsStatus::CompletedStatus { total_size, .. } => {
                        yield Progression::ProgressComplete {
                            name: attachment.name(),
                            total: total_size,
                        };
                    },
                    rust_ipfs::unixfs::UnixfsStatus::FailedStatus { written, error, .. } => {
                        if let Err(e) = tokio::fs::remove_file(&path).await {
                            error!("Error removing file: {e}");
                        }
                        yield Progression::ProgressFailed {
                            name: attachment.name(),
                            last_size: Some(written),
                            error: error.map(|e| e.to_string()),
                        };
                    },
                }
            }
        };

        Ok(progress_stream.boxed())
    }

    pub async fn download_stream(
        &self,
        conversation: Uuid,
        message_id: Uuid,
        file: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        let members = self.get(conversation).await.map(|c| {
            c.recipients()
                .iter()
                .filter_map(|did| did.to_peer_id().ok())
                .collect::<Vec<_>>()
        })?;

        let message = self.get_message(conversation, message_id).await?;

        if message.message_type() != MessageType::Attachment {
            return Err(Error::InvalidMessage);
        }

        let attachment = message
            .attachments()
            .iter()
            .find(|attachment| attachment.name() == file)
            .cloned()
            .ok_or(Error::FileNotFound)?;

        let reference = attachment
            .reference()
            .and_then(|reference| IpfsPath::from_str(&reference).ok())
            .ok_or(Error::FileNotFound)?;

        let ipfs = self.ipfs.clone();

        let stream = async_stream::stream! {
            let stream = ipfs.unixfs().cat(reference, None, &members, false, None);

            for await result in stream {
                let result = result.map_err(anyhow::Error::from).map_err(Error::from);
                yield result;
            }
        };

        Ok(stream.boxed())
    }

    pub async fn add_restricted(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;

        if matches!(conversation.conversation_type, ConversationType::Direct) {
            return Err(Error::InvalidConversation);
        }

        let Some(creator) = conversation.creator.clone() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if !self.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsntBlocked);
        }

        debug_assert!(!conversation.recipients.contains(did_key));
        debug_assert!(!conversation.restrict.contains(did_key));

        conversation.restrict.push(did_key.clone());

        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let event = MessagingEvents::UpdateConversation {
            conversation,
            kind: ConversationUpdateKind::AddRestricted {
                did: did_key.clone(),
            },
        };

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn remove_restricted(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;

        if matches!(conversation.conversation_type, ConversationType::Direct) {
            return Err(Error::InvalidConversation);
        }

        let Some(creator) = conversation.creator.clone() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if self.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        debug_assert!(conversation.restrict.contains(did_key));

        conversation
            .restrict
            .retain(|restricted| restricted != did_key);

        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let event = MessagingEvents::UpdateConversation {
            conversation,
            kind: ConversationUpdateKind::RemoveRestricted {
                did: did_key.clone(),
            },
        };

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn update_conversation_name(
        &mut self,
        conversation_id: Uuid,
        name: &str,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let tx = self.subscribe(conversation_id).await?;

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

        if matches!(conversation.conversation_type, ConversationType::Direct) {
            return Err(Error::InvalidConversation);
        }

        let Some(creator) = conversation.creator.as_ref() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        conversation.name = (!name.is_empty()).then_some(name.to_string());

        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let new_name = conversation.name();

        let event = MessagingEvents::UpdateConversation {
            conversation,
            kind: ConversationUpdateKind::ChangeName { name: new_name },
        };

        let _ = tx.send(MessageEventKind::ConversationNameUpdated {
            conversation_id,
            name: name.to_string(),
        });

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn add_recipient(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;

        let settings = match conversation.settings {
            ConversationSettings::Group(settings) => settings,
            ConversationSettings::Direct(_) => return Err(Error::InvalidConversation),
        };
        assert_eq!(conversation.conversation_type, ConversationType::Group);

        let Some(creator) = conversation.creator.clone() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if !settings.members_can_add_participants() && creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if self.root.is_blocked(did_key).await? {
            return Err(Error::PublicKeyIsBlocked);
        }

        if conversation.recipients.contains(did_key) {
            return Err(Error::IdentityExist);
        }

        conversation.recipients.push(did_key.clone());

        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: conversation.clone(),
            kind: ConversationUpdateKind::AddParticipant {
                did: did_key.clone(),
            },
        };

        let tx = self.subscribe(conversation_id).await?;
        let _ = tx.send(MessageEventKind::RecipientAdded {
            conversation_id,
            recipient: did_key.clone(),
        });

        self.publish(conversation_id, None, event, true).await?;

        let new_event = ConversationEvents::NewGroupConversation { conversation };

        self.send_single_conversation_event(conversation_id, did_key, new_event)
            .await?;
        if let Err(_e) = self.request_key(conversation_id, did_key).await {}
        Ok(())
    }

    pub async fn remove_recipient(
        &mut self,
        conversation_id: Uuid,
        did_key: &DID,
        broadcast: bool,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;

        if matches!(conversation.conversation_type, ConversationType::Direct) {
            return Err(Error::InvalidConversation);
        }

        let Some(creator) = conversation.creator.as_ref() else {
            return Err(Error::InvalidConversation);
        };

        let own_did = &*self.keypair;

        if creator.ne(own_did) {
            return Err(Error::PublicKeyInvalid);
        }

        if creator.eq(did_key) {
            return Err(Error::PublicKeyInvalid);
        }

        if !conversation.recipients.contains(did_key) {
            return Err(Error::IdentityDoesntExist);
        }

        conversation.recipients.retain(|did| did.ne(did_key));
        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;

        let event = MessagingEvents::UpdateConversation {
            conversation: conversation.clone(),
            kind: ConversationUpdateKind::RemoveParticipant {
                did: did_key.clone(),
            },
        };

        let tx = self.subscribe(conversation_id).await?;
        let _ = tx.send(MessageEventKind::RecipientRemoved {
            conversation_id,
            recipient: did_key.clone(),
        });

        self.publish(conversation_id, None, event, true).await?;

        if broadcast {
            let new_event = ConversationEvents::DeleteConversation { conversation_id };

            self.send_single_conversation_event(conversation_id, did_key, new_event)
                .await?;
        }

        Ok(())
    }

    pub async fn delete_conversation(
        &mut self,
        conversation_id: Uuid,
        broadcast: bool,
    ) -> Result<(), Error> {
        self.destroy_conversation(conversation_id).await;

        let document_type = self.delete(conversation_id).await?;

        if broadcast {
            let recipients = document_type.recipients();

            let mut can_broadcast = true;

            if matches!(
                document_type.conversation_type,
                ConversationType::Group { .. }
            ) {
                let own_did = &*self.keypair;
                let creator = document_type
                    .creator
                    .as_ref()
                    .ok_or(Error::InvalidConversation)?;

                if creator.ne(own_did) {
                    can_broadcast = false;
                    let recipients = recipients
                        .iter()
                        .filter(|did| own_did.ne(did))
                        .filter(|did| creator.ne(did))
                        .cloned()
                        .collect::<Vec<_>>();
                    if let Err(e) = self
                        .leave_group_conversation(creator, &recipients, conversation_id)
                        .await
                    {
                        error!("Error leaving conversation: {e}");
                    }
                }
            }

            let own_did = &*self.keypair;

            if can_broadcast {
                let peer_id_list = recipients
                    .clone()
                    .iter()
                    .filter(|did| own_did.ne(did))
                    .map(|did| (did.clone(), did))
                    .filter_map(|(a, b)| b.to_peer_id().map(|pk| (a, pk)).ok())
                    .collect::<Vec<_>>();

                let event = serde_json::to_vec(&ConversationEvents::DeleteConversation {
                    conversation_id: document_type.id(),
                })?;

                let main_timer = Instant::now();
                for (recipient, peer_id) in peer_id_list {
                    let bytes = ecdh_encrypt(own_did, Some(&recipient), &event)?;
                    let signature = sign_serde(own_did, &bytes)?;

                    let payload = Payload::new(own_did, &bytes, &signature);

                    let peers = self.ipfs.pubsub_peers(Some(recipient.messaging())).await?;
                    let timer = Instant::now();
                    let mut time = true;
                    if !peers.contains(&peer_id)
                        || (peers.contains(&peer_id)
                            && self
                                .ipfs
                                .pubsub_publish(recipient.messaging(), payload.to_bytes()?.into())
                                .await
                                .is_err())
                    {
                        warn!("Unable to publish to topic. Queuing event");
                        //Note: If the error is related to peer not available then we should push this to queue but if
                        //      its due to the message limit being reached we should probably break up the message to fix into
                        //      "max_transmit_size" within rust-libp2p gossipsub
                        //      For now we will queue the message if we hit an error
                        // if let Err(e) = self
                        //     .queue_event(
                        //         recipient.clone(),
                        //         Queue::direct(
                        //             document_type.id(),
                        //             None,
                        //             peer_id,
                        //             recipient.messaging(),
                        //             payload.data().to_vec(),
                        //         ),
                        //     )
                        //     .await
                        // {
                        //     error!("Error submitting event to queue: {e}");
                        // }
                        time = false;
                    }

                    if time {
                        let end = timer.elapsed();
                        tracing::info!("Event sent to {recipient}");
                        tracing::trace!("Took {}ms to send event", end.as_millis());
                    }
                }
                let main_timer_end = main_timer.elapsed();
                tracing::trace!(
                    "Completed processing within {}ms",
                    main_timer_end.as_millis()
                );
            }
        }

        let conversation_id = document_type.id();

        self.event
            .emit(RayGunEventKind::ConversationDeleted { conversation_id })
            .await;

        Ok(())
    }

    async fn leave_group_conversation(
        &mut self,
        creator: &DID,
        list: &[DID],
        conversation_id: Uuid,
    ) -> Result<(), Error> {
        let own_did = &*self.keypair;

        let context = format!("exclude {}", own_did);
        let signature = sign_serde(own_did, &context)?;
        let signature = bs58::encode(signature).into_string();

        let event = ConversationEvents::LeaveConversation {
            conversation_id,
            recipient: own_did.clone(),
            signature,
        };

        //We want to send the event to the recipients until the creator can remove them from the conversation directly

        for did in list.iter() {
            if let Err(e) = self
                .send_single_conversation_event(conversation_id, did, event.clone())
                .await
            {
                tracing::error!("Error sending conversation event to {did}: {e}");
                continue;
            }
        }

        self.send_single_conversation_event(conversation_id, creator, event)
            .await
    }

    pub async fn send_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        let conversation = self.get(conversation_id).await?;
        let own_did = &*self.keypair;

        let event = MessagingEvents::Event {
            conversation_id: conversation.id(),
            member: own_did.clone(),
            event,
            cancelled: false,
        };
        self.send_message_event(conversation_id, event).await
    }

    pub async fn cancel_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        let conversation = self.get(conversation_id).await?;
        let own_did = &*self.keypair;

        let event = MessagingEvents::Event {
            conversation_id: conversation.id(),
            member: own_did.clone(),
            event,
            cancelled: true,
        };
        self.send_message_event(conversation_id, event).await
    }

    pub async fn send_message_event(
        &mut self,
        conversation_id: Uuid,
        event: MessagingEvents,
    ) -> Result<(), Error> {
        let conversation = self.get(conversation_id).await?;

        let own_did = &*self.keypair;

        let event = serde_json::to_vec(&event)?;

        let bytes = match conversation.conversation_type {
            ConversationType::Direct => {
                let recipient = conversation
                    .recipients()
                    .iter()
                    .filter(|did| own_did.ne(did))
                    .cloned()
                    .collect::<Vec<_>>()
                    .first()
                    .cloned()
                    .ok_or(Error::InvalidConversation)?;
                ecdh_encrypt(own_did, Some(&recipient), &event)?
            }
            ConversationType::Group { .. } => {
                let keystore = self.get_keystore(conversation.id()).await?;
                let key = keystore.get_latest(own_did, own_did)?;
                Cipher::direct_encrypt(&event, &key)?
            }
        };

        let signature = sign_serde(own_did, &bytes)?;
        let payload = Payload::new(own_did, &bytes, &signature);

        let peers = self
            .ipfs
            .pubsub_peers(Some(conversation.event_topic()))
            .await?;

        if !peers.is_empty() {
            if let Err(e) = self
                .ipfs
                .pubsub_publish(conversation.event_topic(), payload.to_bytes()?.into())
                .await
            {
                error!("Unable to send event: {e}");
            }
        }
        Ok(())
    }

    pub async fn update_conversation_settings(
        &mut self,
        conversation_id: Uuid,
        settings: ConversationSettings,
    ) -> Result<(), Error> {
        let mut conversation = self.get(conversation_id).await?;
        let own_did = &*self.keypair;
        let Some(creator) = &conversation.creator else {
            return Err(Error::InvalidConversation);
        };
        if creator != own_did {
            return Err(Error::PublicKeyInvalid);
        }

        conversation.settings = settings;
        self.set_document(conversation).await?;

        let conversation = self.get(conversation_id).await?;
        let event = MessagingEvents::UpdateConversation {
            conversation: conversation.clone(),
            kind: ConversationUpdateKind::ChangeSettings {
                settings: conversation.settings,
            },
        };

        let tx = self.subscribe(conversation_id).await?;
        let _ = tx.send(MessageEventKind::ConversationSettingsUpdated {
            conversation_id,
            settings: conversation.settings,
        });

        self.publish(conversation_id, None, event, true).await
    }

    pub async fn publish(
        &mut self,
        conversation: Uuid,
        _message_id: Option<Uuid>,
        event: MessagingEvents,
        _queue: bool,
    ) -> Result<(), Error> {
        let conversation = self.get(conversation).await?;

        let own_did = &*self.keypair;

        let event = serde_json::to_vec(&event)?;

        let bytes = match conversation.conversation_type {
            ConversationType::Direct => {
                let recipient = conversation
                    .recipients()
                    .iter()
                    .filter(|did| own_did.ne(did))
                    .cloned()
                    .collect::<Vec<_>>()
                    .first()
                    .cloned()
                    .ok_or(Error::InvalidConversation)?;
                ecdh_encrypt(own_did, Some(&recipient), &event)?
            }
            ConversationType::Group { .. } => {
                let keystore = self.get_keystore(conversation.id()).await?;
                let key = keystore.get_latest(own_did, own_did)?;
                Cipher::direct_encrypt(&event, &key)?
            }
        };

        let signature = sign_serde(own_did, &bytes)?;

        let payload = Payload::new(own_did, &bytes, &signature);

        let peers = self.ipfs.pubsub_peers(Some(conversation.topic())).await?;

        let mut can_publish = false;

        for recipient in conversation
            .recipients()
            .iter()
            .filter(|did| own_did.ne(did))
        {
            let peer_id = recipient.to_peer_id()?;

            // We want to confirm that there is atleast one peer subscribed before attempting to send a message
            match peers.contains(&peer_id) {
                true => {
                    can_publish = true;
                }
                false => {
                    // if queue {
                    //     if let Err(e) = self
                    //         .queue_event(
                    //             recipient.clone(),
                    //             Queue::direct(
                    //                 conversation.id(),
                    //                 message_id,
                    //                 peer_id,
                    //                 conversation.topic(),
                    //                 payload.data().to_vec(),
                    //             ),
                    //         )
                    //         .await
                    //     {
                    //         error!("Error submitting event to queue: {e}");
                    //     }
                    // }
                }
            };
        }

        if can_publish {
            let bytes = payload.to_bytes()?;
            tracing::trace!("Payload size: {} bytes", bytes.len());
            let timer = Instant::now();
            let mut time = true;
            if let Err(_e) = self
                .ipfs
                .pubsub_publish(conversation.topic(), bytes.into())
                .await
            {
                error!("Error publishing: {_e}");
                time = false;
            }
            if time {
                let end = timer.elapsed();
                tracing::trace!("Took {}ms to send event", end.as_millis());
            }
        }

        Ok(())
    }

    async fn send_single_conversation_event(
        &mut self,
        _conversation_id: Uuid,
        did_key: &DID,
        event: ConversationEvents,
    ) -> Result<(), Error> {
        let event = serde_json::to_vec(&event)?;

        let bytes = ecdh_encrypt(&self.keypair, Some(did_key), &event)?;
        let signature = sign_serde(&self.keypair, &bytes)?;

        let payload = Payload::new(&self.keypair, &bytes, &signature);

        let peer_id = did_key.to_peer_id()?;
        let peers = self.ipfs.pubsub_peers(Some(did_key.messaging())).await?;

        let mut time = true;
        let timer = Instant::now();
        if !peers.contains(&peer_id)
            || (peers.contains(&peer_id)
                && self
                    .ipfs
                    .pubsub_publish(did_key.messaging(), payload.to_bytes()?.into())
                    .await
                    .is_err())
        {
            warn!("Unable to publish to topic. Queuing event");
            // if let Err(e) = self
            //     .queue_event(
            //         did_key.clone(),
            //         Queue::direct(
            //             conversation_id,
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
            time = false;
        }
        if time {
            let end = timer.elapsed();
            tracing::info!("Event sent to {did_key}");
            tracing::trace!("Took {}ms to send event", end.as_millis());
        }

        Ok(())
    }

    async fn destroy_conversation(&mut self, conversation_id: Uuid) {
        self.topic_stream.remove(&conversation_id);
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

            this.event
                .emit(RayGunEventKind::ConversationCreated { conversation_id })
                .await;
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

            let conversation = this.get(conversation_id).await?;

            tracing::info!(%conversation_id,"{} conversation created", conversation_type);
            let keypair = this.keypair.clone();

            for recipient in conversation.recipients.iter().filter(|d| (*keypair).ne(d)) {
                if let Err(e) = this.request_key(conversation_id, recipient).await {
                    tracing::warn!("Failed to send exchange request to {recipient}: {e}");
                }
            }

            this.event
                .emit(RayGunEventKind::ConversationCreated { conversation_id })
                .await;
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
                this.remove_recipient(conversation_id, &recipient, false)
                    .await?;
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
                    let tx = this.subscribe(conversation_id).await?;
                    if let Err(e) = tx.send(MessageEventKind::RecipientRemoved {
                        conversation_id,
                        recipient,
                    }) {
                        tracing::error!("Error broadcasting event: {e}");
                    }
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

            this.event
                .emit(RayGunEventKind::ConversationDeleted { conversation_id })
                .await;
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

async fn process_identity_events(
    this: &mut ConversationTask,
    event: MultiPassEventKind,
) -> Result<(), Error> {
    //TODO: Tie this into a configuration
    let with_friends = false;

    match event {
        MultiPassEventKind::FriendAdded { did } => {
            if !with_friends {
                return Ok(());
            }

            match this.create_conversation(&did).await {
                Ok(_) | Err(Error::ConversationExist { .. }) => return Ok(()),
                Err(e) => return Err(e),
            }
        }

        MultiPassEventKind::Blocked { did } | MultiPassEventKind::BlockedBy { did } => {
            let list = this.list().await;

            for conversation in list.iter().filter(|c| c.recipients().contains(&did)) {
                let id = conversation.id();
                match conversation.conversation_type {
                    ConversationType::Direct => {
                        if let Err(e) = this.delete_conversation(id, true).await {
                            warn!("Failed to delete conversation {id}: {e}");
                            continue;
                        }
                    }
                    ConversationType::Group { .. } => {
                        if conversation.creator != Some((*this.keypair).clone()) {
                            continue;
                        }

                        if let Err(e) = this.remove_recipient(id, &did, true).await {
                            warn!("Failed to remove {did} from conversation {id}: {e}");
                            continue;
                        }

                        if this.root.is_blocked(&did).await.unwrap_or_default() {
                            _ = this.add_restricted(id, &did).await;
                        }
                    }
                }
            }
        }
        MultiPassEventKind::Unblocked { did } => {
            let own_did = (*this.keypair).clone();
            let list = this.list().await;

            for conversation in list
                .iter()
                .filter(|c| {
                    c.creator
                        .as_ref()
                        .map(|creator| own_did.eq(creator))
                        .unwrap_or_default()
                })
                .filter(|c| c.conversation_type == ConversationType::Group)
                .filter(|c| c.restrict.contains(&did))
            {
                let id = conversation.id();
                _ = this.remove_restricted(id, &did).await;
            }
        }
        MultiPassEventKind::FriendRemoved { did } => {
            if !with_friends {
                return Ok(());
            }

            let list = this.list().await;

            for conversation in list.iter().filter(|c| c.recipients().contains(&did)) {
                let id = conversation.id();
                match conversation.conversation_type {
                    ConversationType::Direct => {
                        if let Err(e) = this.delete_conversation(id, true).await {
                            warn!("Failed to delete conversation {id}: {e}");
                            continue;
                        }
                    }
                    ConversationType::Group { .. } => {
                        if conversation.creator != Some((*this.keypair).clone()) {
                            continue;
                        }

                        if let Err(e) = this.remove_recipient(id, &did, true).await {
                            warn!("Failed to remove {did} from conversation {id}: {e}");
                            continue;
                        }
                    }
                }
            }
        }
        MultiPassEventKind::IdentityOnline { .. } => {
            //TODO: Check queue and process any entry once peer is subscribed to the respective topics.
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

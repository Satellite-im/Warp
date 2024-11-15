use crate::store::conversation::message::MessageDocument;
use crate::store::files::FileStore;
use crate::store::keystore::Keystore;
use crate::store::message::CHAT_DIRECTORY;
use crate::store::{MAX_MESSAGE_SIZE, MIN_MESSAGE_SIZE};
use either::Either;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::stream::SelectAll;
use futures::{stream, FutureExt, SinkExt, Stream, StreamExt};
use rust_ipfs::{Ipfs, Keypair};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};
use uuid::Uuid;
use warp::constellation::directory::Directory;
use warp::constellation::file::File;
use warp::constellation::{ConstellationProgressStream, Progression};
use warp::crypto::DID;
use warp::error::Error;
use warp::raygun::{AttachmentKind, Location, LocationKind, MessageType};

type AOneShot = (MessageDocument, oneshot::Sender<Result<(), Error>>);
type ProgressedStream = BoxStream<'static, (LocationKind, Progression, Option<File>)>;
pub struct AttachmentStream {
    ipfs: Ipfs,
    keypair: Keypair,
    local_did: DID,
    attachment_tx: futures::channel::mpsc::Sender<AOneShot>,
    conversation_id: Uuid,
    message_id: Uuid,
    reply_to: Option<Uuid>,
    locations: Vec<Location>,
    directory: Directory,
    lines: Option<Vec<String>>,
    keystore: Either<DID, Keystore>,
    file_store: FileStore,
    state: AttachmentState,

    progressed: Option<SelectAll<ProgressedStream>>,
    successful_attachment: Vec<File>,
}

enum AttachmentState {
    Initialize,
    Pending,
    Final {
        fut: BoxFuture<'static, Result<(), Error>>,
    },
    Complete,
}

impl AttachmentStream {
    pub fn new(
        ipfs: &Ipfs,
        keypair: &Keypair,
        local_did: &DID,
        file_store: &FileStore,
        conversation_id: Uuid,
        keystore: Either<DID, Keystore>,
        attachment_tx: futures::channel::mpsc::Sender<AOneShot>,
    ) -> Self {
        Self {
            ipfs: ipfs.clone(),
            keypair: keypair.clone(),
            local_did: local_did.clone(),
            file_store: file_store.clone(),
            conversation_id,
            message_id: Uuid::new_v4(),
            directory: Directory::default(),
            keystore,
            attachment_tx,
            reply_to: None,
            locations: Vec::new(),
            lines: None,
            state: AttachmentState::Initialize,
            progressed: Some(SelectAll::new()),
            successful_attachment: Vec::new(),
        }
    }

    pub fn message_id(&self) -> Uuid {
        self.message_id
    }

    pub fn set_lines(mut self, lines: Vec<String>) -> Result<Self, Error> {
        if lines.is_empty() {
            return Ok(self);
        }

        let lines_value_length: usize = lines
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim())
            .map(|s| s.chars().count())
            .sum();

        if lines_value_length < MIN_MESSAGE_SIZE {
            tracing::error!(
                current_size = lines_value_length,
                min = MIN_MESSAGE_SIZE,
                "length of message is invalid"
            );
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: None,
                maximum: Some(MIN_MESSAGE_SIZE),
            });
        }

        if lines_value_length > MAX_MESSAGE_SIZE {
            tracing::error!(
                current_size = lines_value_length,
                max = MAX_MESSAGE_SIZE,
                "length of message is invalid"
            );
            return Err(Error::InvalidLength {
                context: "message".into(),
                current: lines_value_length,
                minimum: None,
                maximum: Some(MAX_MESSAGE_SIZE),
            });
        }

        self.lines = Some(lines);
        Ok(self)
    }

    pub fn set_locations(mut self, locations: Vec<Location>) -> Result<Self, Error> {
        let conversation_id = self.conversation_id;

        if locations.len() > 32 {
            return Err(Error::InvalidLength {
                context: "files".into(),
                current: locations.len(),
                minimum: Some(1),
                maximum: Some(32),
            });
        }

        let files = locations
            .into_iter()
            .filter(|location| match location {
                Location::Disk { path } => path.is_file(),
                _ => true,
            })
            .collect::<Vec<_>>();

        if files.is_empty() {
            return Err(Error::NoAttachments);
        }

        let root_directory = self.file_store.root_directory();

        if !root_directory.has_item(CHAT_DIRECTORY) {
            let new_dir = Directory::new(CHAT_DIRECTORY);
            root_directory.add_directory(new_dir)?;
        }

        let mut media_dir = root_directory
            .get_last_directory_from_path(&format!("/{CHAT_DIRECTORY}/{conversation_id}"))?;

        // if the directory that returned is the chat directory, this means we should create
        // the directory specific to the conversation
        if media_dir.name() == CHAT_DIRECTORY {
            let new_dir = Directory::new(&conversation_id.to_string());
            media_dir.add_directory(new_dir)?;
            media_dir = media_dir.get_last_directory_from_path(&conversation_id.to_string())?;
        }

        assert_eq!(media_dir.name(), conversation_id.to_string());
        self.locations = files;
        self.directory = media_dir;
        Ok(self)
    }

    pub fn set_reply(mut self, message_id: impl Into<Option<Uuid>>) -> Self {
        self.reply_to = message_id.into();
        self
    }
}

impl Stream for AttachmentStream {
    type Item = AttachmentKind;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if matches!(this.state, AttachmentState::Complete) {
            return Poll::Ready(None);
        }

        let conversation_id = this.conversation_id;

        loop {
            match this.state {
                AttachmentState::Initialize => {
                    let progressed = this.progressed.as_mut().expect("not terminated");
                    let mut in_stack = vec![];
                    while let Some(file) = this.locations.pop() {
                        let kind = LocationKind::from(&file);
                        match file {
                            Location::Constellation { path } => {
                                match this
                                    .file_store
                                    .root_directory()
                                    .get_item_by_path(&path)
                                    .and_then(|item| item.get_file())
                                {
                                    Ok(f) => {
                                        progressed.push(Box::pin(stream::once(async {
                                            (
                                                kind,
                                                Progression::ProgressComplete {
                                                    name: f.name(),
                                                    total: Some(f.size()),
                                                },
                                                Some(f),
                                            )
                                        })));
                                    }
                                    Err(e) => {
                                        let constellation_path = PathBuf::from(&path);
                                        let name = constellation_path
                                            .file_name()
                                            .and_then(OsStr::to_str)
                                            .map(str::to_string)
                                            .unwrap_or(path.to_string());
                                        progressed.push(Box::pin(stream::once(async {
                                            (
                                                kind,
                                                Progression::ProgressFailed {
                                                    name,
                                                    last_size: None,
                                                    error: e,
                                                },
                                                None,
                                            )
                                        })));
                                    }
                                }
                            }
                            Location::Stream {
                                name,
                                size,
                                stream: bytes_st,
                            } => {
                                let mut filename = name;

                                let original = filename.clone();

                                let current_directory = this.directory.clone();

                                let mut interval = 0;
                                let skip;
                                loop {
                                    if in_stack.contains(&filename)
                                        || current_directory.has_item(&filename)
                                    {
                                        if interval > 2000 {
                                            skip = true;
                                            break;
                                        }
                                        interval += 1;
                                        let file = PathBuf::from(&original);
                                        let file_stem = file
                                            .file_stem()
                                            .and_then(OsStr::to_str)
                                            .map(str::to_string);
                                        let ext = file
                                            .extension()
                                            .and_then(OsStr::to_str)
                                            .map(str::to_string);

                                        filename = match (file_stem, ext) {
                                            (Some(filename), Some(ext)) => {
                                                format!("{filename} ({interval}).{ext}")
                                            }
                                            _ => format!("{original} ({interval})"),
                                        };
                                        continue;
                                    }
                                    skip = false;
                                    break;
                                }

                                if skip {
                                    progressed.push(Box::pin(stream::once(async {
                                        (
                                            kind,
                                            Progression::ProgressFailed {
                                                name: filename,
                                                last_size: None,
                                                error: Error::InvalidFile,
                                            },
                                            None,
                                        )
                                    })));
                                    continue;
                                }

                                in_stack.push(filename.clone());

                                let filename =
                                    format!("/{CHAT_DIRECTORY}/{conversation_id}/{filename}");

                                let st = ConstellationFutureStream::Future {
                                    name: filename.clone(),
                                    fut: {
                                        let mut filestore = this.file_store.clone();
                                        let filename = filename.to_string();
                                        Box::pin(async move {
                                            filestore.put_stream(&filename, size, bytes_st).await
                                        })
                                    },
                                };

                                let directory = this.file_store.root_directory();

                                let st = BindKey::new((directory, filename, kind), st)
                                    .map(|((directory, filename, kind), progress)| match progress {
                                        item @ Progression::CurrentProgress { .. } => {
                                            (kind, item, None)
                                        }
                                        item @ Progression::ProgressComplete { .. } => {
                                            let file_name = directory
                                                .get_item_by_path(&filename)
                                                .and_then(|item| item.get_file())
                                                .ok();
                                            (kind, item, file_name)
                                        }
                                        item @ Progression::ProgressFailed { .. } => {
                                            (kind, item, None)
                                        }
                                    })
                                    .boxed();

                                progressed.push(st);
                            }
                            Location::Disk { .. } if cfg!(target_arch = "wasm32") => {
                                unreachable!("cannot access filesystem with web assembly")
                            }
                            Location::Disk { path } => {
                                let mut filename = match path.file_name() {
                                    Some(file) => file.to_string_lossy().to_string(),
                                    None => continue,
                                };

                                let original = filename.clone();

                                let current_directory = this.directory.clone();

                                let mut interval = 0;
                                let skip;
                                loop {
                                    if in_stack.contains(&filename)
                                        || current_directory.has_item(&filename)
                                    {
                                        if interval > 2000 {
                                            skip = true;
                                            break;
                                        }
                                        interval += 1;
                                        let file = PathBuf::from(&original);
                                        let file_stem = file
                                            .file_stem()
                                            .and_then(OsStr::to_str)
                                            .map(str::to_string);
                                        let ext = file
                                            .extension()
                                            .and_then(OsStr::to_str)
                                            .map(str::to_string);

                                        filename = match (file_stem, ext) {
                                            (Some(filename), Some(ext)) => {
                                                format!("{filename} ({interval}).{ext}")
                                            }
                                            _ => format!("{original} ({interval})"),
                                        };
                                        continue;
                                    }
                                    skip = false;
                                    break;
                                }

                                if skip {
                                    progressed.push(Box::pin(stream::once(async {
                                        (
                                            kind,
                                            Progression::ProgressFailed {
                                                name: filename,
                                                last_size: None,
                                                error: Error::InvalidFile,
                                            },
                                            None,
                                        )
                                    })));
                                    continue;
                                }

                                let file_path = path.display().to_string();

                                in_stack.push(filename.clone());

                                let filename =
                                    format!("/{CHAT_DIRECTORY}/{conversation_id}/{filename}");

                                let st = ConstellationFutureStream::Future {
                                    name: filename.clone(),
                                    fut: {
                                        let mut filestore = this.file_store.clone();
                                        let filename = filename.to_string();
                                        let file_path = file_path.clone();
                                        Box::pin(async move {
                                            filestore.put(&filename, &file_path).await
                                        })
                                    },
                                };

                                let directory = this.file_store.root_directory();

                                let st = BindKey::new((directory, filename, kind), st)
                                    .map(|((directory, filename, kind), progress)| match progress {
                                        item @ Progression::CurrentProgress { .. } => {
                                            (kind, item, None)
                                        }
                                        item @ Progression::ProgressComplete { .. } => {
                                            let file_name = directory
                                                .get_item_by_path(&filename)
                                                .and_then(|item| item.get_file())
                                                .ok();
                                            (kind, item, file_name)
                                        }
                                        item @ Progression::ProgressFailed { .. } => {
                                            (kind, item, None)
                                        }
                                    })
                                    .boxed();

                                progressed.push(st);
                            }
                        }
                    }
                    if progressed.is_empty() {
                        this.state = AttachmentState::Complete;
                        let kind = AttachmentKind::Pending(Err(Error::NoAttachments));
                        return Poll::Ready(Some(kind));
                    }

                    this.state = AttachmentState::Pending;
                }
                AttachmentState::Pending => {
                    let progressed = match this.progressed.as_mut() {
                        Some(st) => st,
                        None => unreachable!("cannot repoll terminated stream"),
                    };
                    match futures::ready!(progressed.poll_next_unpin(cx)) {
                        Some((kind, progress, file)) => {
                            let attachment_kind = AttachmentKind::AttachedProgress(kind, progress);
                            if let Some(file) = file {
                                this.successful_attachment.push(file);
                            }
                            return Poll::Ready(Some(attachment_kind));
                        }
                        None => {
                            tracing::error!(?this.successful_attachment);
                            if this.successful_attachment.is_empty() {
                                this.state = AttachmentState::Complete;
                                let kind = AttachmentKind::Pending(Err(Error::NoAttachments));
                                return Poll::Ready(Some(kind));
                            }
                            let attachments = std::mem::take(&mut this.successful_attachment);
                            let messages = std::mem::take(&mut this.lines);
                            let reply_id = this.reply_to;
                            let message_id = this.message_id;
                            let local_did = this.local_did.clone();
                            let ipfs = this.ipfs.clone();
                            let keystore = this.keystore.clone();
                            let keypair = this.keypair.clone();
                            let mut atx = this.attachment_tx.clone();
                            let final_fut = async move {
                                let mut message = warp::raygun::Message::default();
                                message.set_id(message_id);
                                message.set_message_type(MessageType::Attachment);
                                message.set_conversation_id(conversation_id);
                                message.set_sender(local_did);
                                message.set_attachment(attachments);
                                if let Some(messages) = messages {
                                    message.set_lines(messages);
                                }
                                message.set_replied(reply_id);

                                let message = MessageDocument::new(
                                    &ipfs,
                                    &keypair,
                                    message,
                                    keystore.as_ref(),
                                )
                                .await?;

                                let (tx, rx) = oneshot::channel();
                                _ = atx.send((message, tx)).await;

                                rx.await.expect("active task")
                            };
                            this.state = AttachmentState::Final {
                                fut: Box::pin(final_fut),
                            };
                            this.progressed.take();
                        }
                    }
                }
                AttachmentState::Final { ref mut fut } => {
                    let result = futures::ready!(fut.poll_unpin(cx));
                    this.state = AttachmentState::Complete;
                    let kind = AttachmentKind::Pending(result);
                    return Poll::Ready(Some(kind));
                }
                AttachmentState::Complete => {
                    return Poll::Ready(None);
                }
            }
        }
    }
}

enum ConstellationFutureStream {
    Future {
        name: String,
        fut: BoxFuture<'static, Result<ConstellationProgressStream, Error>>,
    },
    Stream {
        st: ConstellationProgressStream,
    },
    Done,
}

impl Stream for ConstellationFutureStream {
    type Item = Progression;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if matches!(this, ConstellationFutureStream::Done) {
            return Poll::Ready(None);
        }

        loop {
            match this {
                ConstellationFutureStream::Future { name, fut } => {
                    let st = match futures::ready!(fut.poll_unpin(cx)) {
                        Ok(st) => st,
                        Err(e) => {
                            let name = name.clone();
                            *this = ConstellationFutureStream::Done;
                            return Poll::Ready(Some(Progression::ProgressFailed {
                                name,
                                last_size: None,
                                error: e,
                            }));
                        }
                    };

                    *this = ConstellationFutureStream::Stream { st };
                }
                ConstellationFutureStream::Stream { ref mut st } => {
                    match futures::ready!(Pin::new(st).poll_next(cx)) {
                        Some(item) => return Poll::Ready(Some(item)),
                        None => {
                            *this = ConstellationFutureStream::Done;
                        }
                    }
                }
                ConstellationFutureStream::Done => {
                    return Poll::Ready(None);
                }
            }
        }
    }
}

struct BindKey<K, S> {
    item: K,
    stream: Option<S>,
}

impl<K, S> BindKey<K, S> {
    fn new(item: K, stream: S) -> Self {
        Self {
            item,
            stream: Some(stream),
        }
    }
}

impl<K: Clone + Unpin, S: Stream + Unpin> Stream for BindKey<K, S> {
    type Item = (K, S::Item);
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let Some(st) = this.stream.as_mut() else {
            return Poll::Ready(None);
        };

        let item = match futures::ready!(Pin::new(st).poll_next(cx)) {
            Some(item) => item,
            None => {
                this.stream.take();
                return Poll::Ready(None);
            }
        };

        Poll::Ready(Some((this.item.clone(), item)))
    }
}

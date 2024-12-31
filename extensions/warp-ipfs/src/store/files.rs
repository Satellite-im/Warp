#[cfg(not(target_arch = "wasm32"))]
use std::ffi::OsStr;

use async_rt::AbortableJoinHandle;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{
    channel::{mpsc, oneshot},
    future::BoxFuture,
    stream::BoxStream,
    FutureExt, SinkExt, StreamExt, TryStreamExt,
};
use futures_finally::try_stream::FinallyTryStreamExt;
use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use rust_ipfs::{unixfs::UnixfsStatus, Ipfs, IpfsPath};

use tracing::{Instrument, Span};
use warp::{
    constellation::{
        directory::Directory, ConstellationEventKind, ConstellationProgressStream, Progression,
    },
    error::Error,
};

use parking_lot::RwLock;
use warp::constellation::item::{Item, ItemType};

use super::{
    document::root::RootDocumentMap, event_subscription::EventSubscription,
    MAX_THUMBNAIL_STREAM_SIZE,
};
use crate::{
    config::{self, Config},
    thumbnail::ThumbnailGenerator,
    to_file_type,
};

#[derive(Clone)]
pub struct FileStore {
    index: Directory,
    path: Arc<RwLock<PathBuf>>,
    config: config::Config,
    command_sender: mpsc::Sender<FileTaskCommand>,
    _handle: AbortableJoinHandle<()>,
}

impl FileStore {
    pub async fn new(
        ipfs: &Ipfs,
        root: &RootDocumentMap,
        config: &Config,
        constellation_tx: EventSubscription<ConstellationEventKind>,
        span: &Span,
    ) -> Self {
        let config = config.clone();

        let index = Directory::new("root");

        let thumbnail_store = ThumbnailGenerator::new(ipfs);

        let (command_sender, command_receiver) = futures::channel::mpsc::channel(1);
        let (export_tx, export_rx) = futures::channel::mpsc::channel(0);
        let (signal_tx, signal_rx) = futures::channel::mpsc::unbounded();

        let mut task = FileTask {
            index,
            path: Arc::default(),
            root: root.clone(),
            thumbnail_store,
            ipfs: ipfs.clone(),
            constellation_tx,
            config,
            export_rx,
            export_tx,
            signal_tx,
            signal_rx,
            command_receiver,
        };

        if let Err(e) = task.import_v1().await {
            tracing::warn!("Unable to import index: {e}");
        }

        let mut index = task.index.clone();
        let path = task.path.clone();
        let config = task.config.clone();

        let signal = Some(task.signal_tx.clone());
        index.rebuild_paths(&signal);

        let span = span.clone();

        let _handle =
            async_rt::task::spawn_abortable(async move { task.run().await }.instrument(span));

        FileStore {
            index,
            config,
            path,
            command_sender,
            _handle,
        }
    }
}

impl FileStore {
    pub fn modified(&self) -> DateTime<Utc> {
        self.index.modified()
    }

    pub fn root_directory(&self) -> Directory {
        self.index.clone()
    }

    /// Get the current directory that is mutable.
    pub fn current_directory(&self) -> Result<Directory, Error> {
        self.open_directory(&self.get_path().to_string_lossy())
    }

    /// Returns a mutable directory from the filesystem
    pub fn open_directory(&self, path: &str) -> Result<Directory, Error> {
        match path.trim().is_empty() {
            true => Ok(self.root_directory()),
            false => self
                .root_directory()
                .get_item_by_path(path)
                .and_then(|item| item.get_directory()),
        }
    }

    /// Current size of the file system
    pub fn current_size(&self) -> usize {
        self.root_directory().size()
    }

    pub fn max_size(&self) -> usize {
        self.config.max_storage_size().unwrap_or(1024 * 1024 * 1024)
    }

    pub fn set_path(&mut self, path: PathBuf) {
        *self.path.write() = path;
    }

    pub fn get_path(&self) -> PathBuf {
        PathBuf::from(self.path.read().to_string_lossy().replace('\\', "/"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn put(
        &mut self,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<ConstellationProgressStream, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::Put {
                name: name.into(),
                path: path.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get(
        &self,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<ConstellationProgressStream, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::Get {
                name: name.into(),
                path: path.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn put_buffer(
        &mut self,
        name: impl Into<String>,
        buffer: &[u8],
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::PutBuffer {
                name: name.into(),
                buffer: Bytes::from(Vec::from(buffer)),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)??.await
    }

    pub async fn get_buffer(&self, name: impl Into<String>) -> Result<Bytes, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::GetBuffer {
                name: name.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)??.await
    }

    /// Used to upload file to the filesystem with data from a stream
    pub async fn put_stream(
        &mut self,
        name: impl Into<String>,
        total_size: impl Into<Option<usize>>,
        stream: BoxStream<'static, std::io::Result<Bytes>>,
    ) -> Result<ConstellationProgressStream, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::PutStream {
                name: name.into(),
                total_size: total_size.into(),
                stream,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    /// Used to download data from the filesystem using a stream
    pub async fn get_stream(
        &self,
        name: impl Into<String>,
    ) -> Result<BoxStream<'static, Result<Bytes, std::io::Error>>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::GetStream {
                name: name.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    /// Used to remove data from the filesystem
    pub async fn remove(&mut self, name: impl Into<String>, recursive: bool) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::Remove {
                name: name.into(),
                recursive,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn rename(
        &mut self,
        current: impl Into<String>,
        new: impl Into<String>,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::Rename {
                current: current.into(),
                new: new.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn create_directory(
        &mut self,
        name: impl Into<String>,
        recursive: bool,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::CreateDirectory {
                name: name.into(),
                recursive,
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn sync_ref(&mut self, path: impl Into<String>) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .command_sender
            .clone()
            .send(FileTaskCommand::SyncRef {
                path: path.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)??.await
    }
}

type GetStream = BoxStream<'static, Result<Bytes, std::io::Error>>;
type GetBufferFutResult = BoxFuture<'static, Result<Bytes, Error>>;
enum FileTaskCommand {
    #[cfg(not(target_arch = "wasm32"))]
    Put {
        name: String,
        path: String,
        response: oneshot::Sender<Result<ConstellationProgressStream, Error>>,
    },
    PutBuffer {
        name: String,
        buffer: Bytes,
        response: oneshot::Sender<Result<BoxFuture<'static, Result<(), Error>>, Error>>,
    },
    PutStream {
        name: String,
        total_size: Option<usize>,
        stream: BoxStream<'static, std::io::Result<Bytes>>,
        response: oneshot::Sender<Result<ConstellationProgressStream, Error>>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Get {
        name: String,
        path: String,
        response: oneshot::Sender<Result<ConstellationProgressStream, Error>>,
    },
    GetStream {
        name: String,
        response: oneshot::Sender<Result<GetStream, Error>>,
    },
    GetBuffer {
        name: String,
        response: oneshot::Sender<Result<GetBufferFutResult, Error>>,
    },

    Remove {
        name: String,
        recursive: bool,
        response: oneshot::Sender<Result<(), Error>>,
    },
    Rename {
        current: String,
        new: String,
        response: oneshot::Sender<Result<(), Error>>,
    },
    CreateDirectory {
        name: String,
        recursive: bool,
        response: oneshot::Sender<Result<(), Error>>,
    },
    SyncRef {
        path: String,
        response: oneshot::Sender<Result<BoxFuture<'static, Result<(), Error>>, Error>>,
    },
}

struct FileTask {
    index: Directory,
    path: Arc<RwLock<PathBuf>>,
    root: RootDocumentMap,
    config: config::Config,
    ipfs: Ipfs,
    export_tx: futures::channel::mpsc::Sender<()>,
    export_rx: futures::channel::mpsc::Receiver<()>,
    signal_tx: futures::channel::mpsc::UnboundedSender<()>,
    signal_rx: futures::channel::mpsc::UnboundedReceiver<()>,
    thumbnail_store: ThumbnailGenerator,
    constellation_tx: EventSubscription<ConstellationEventKind>,
    command_receiver: futures::channel::mpsc::Receiver<FileTaskCommand>,
}

impl FileTask {
    async fn run(&mut self) {
        loop {
            tokio::select! {
                biased;
                Some(command) = self.command_receiver.next() => {
                    match command {
                        #[cfg(not(target_arch = "wasm32"))]
                        FileTaskCommand::Put {
                            name,
                            path,
                            response,
                        } => {
                           let _ = response.send(self.put(&name, &path).await);
                        },
                        FileTaskCommand::PutBuffer {
                            name,
                            buffer,
                            response,
                        } => {
                            let _ = response.send(self.put_buffer(name, buffer));
                        },
                        FileTaskCommand::PutStream {
                            name,
                            total_size,
                            stream,
                            response,
                        } => {
                           let _ = response.send(self.put_stream(&name, total_size, stream));
                        },
                        #[cfg(not(target_arch = "wasm32"))]
                        FileTaskCommand::Get {
                            name,
                            path,
                            response,
                        } => {
                            let _ = response.send(self.get(&name, &path));
                        },
                        FileTaskCommand::GetStream { name, response } => {
                            let _ = response.send(self.get_stream(&name));
                        },
                        FileTaskCommand::GetBuffer { name, response } => {
                            let _ = response.send(self.get_buffer(name));
                        },
                        FileTaskCommand::Remove {
                            name,
                            recursive,
                            response,
                        } => {
                            let _ = response.send(self.remove(&name, recursive).await);
                        },
                        FileTaskCommand::Rename {
                            current,
                            new,
                            response,
                        } => {
                            let _ = response.send(self.rename(&current, &new).await);
                        },
                        FileTaskCommand::CreateDirectory {
                            name,
                            recursive,
                            response,
                        } => {
                            let _ = response.send(self.create_directory(&name, recursive).await);
                        },
                        FileTaskCommand::SyncRef { path, response } => {
                            let _ = response.send(self.sync_ref(&path));
                        },
                    }
                },
                Some(_) = self.export_rx.next() => {
                    let _ = self.export().await;
                }
                Some(_) = self.signal_rx.next() => {
                    if let Err(_e) = self.export().await {
                        tracing::error!("Error exporting index: {_e}");
                    }
                },
            }
        }
    }

    async fn import_v1(&self) -> Result<(), Error> {
        let index = self.root.get_directory_index().await?;
        self.index.set_items(index.get_items());
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn export(&self) -> Result<(), Error> {
        tracing::trace!("Exporting index");

        let mut index = self.index.clone();
        let signal = Some(self.signal_tx.clone());

        index.rebuild_paths(&signal);

        self.root.set_directory_index(index).await?;

        tracing::trace!("Index exported");
        Ok(())
    }

    pub fn root_directory(&self) -> Directory {
        self.index.clone()
    }

    /// Get the current directory that is mutable.
    fn current_directory(&self) -> Result<Directory, Error> {
        self.open_directory(&self.get_path().to_string_lossy())
    }

    /// Returns a mutable directory from the filesystem
    fn open_directory(&self, path: &str) -> Result<Directory, Error> {
        match path.trim().is_empty() {
            true => Ok(self.root_directory()),
            false => self
                .root_directory()
                .get_item_by_path(path)
                .and_then(|item| item.get_directory()),
        }
    }

    /// Current size of the file system
    fn current_size(&self) -> usize {
        self.root_directory().size()
    }

    fn max_size(&self) -> usize {
        self.config.max_storage_size().unwrap_or(1024 * 1024 * 1024)
    }

    fn get_path(&self) -> PathBuf {
        PathBuf::from(self.path.read().to_string_lossy().replace('\\', "/"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn put(&mut self, name: &str, path: &str) -> Result<ConstellationProgressStream, Error> {
        let (name, dest_path) = split_file_from_path(name)?;

        let ipfs = self.ipfs.clone();

        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(Error::FileNotFound);
        }

        let file_size = fs::file_size(&path).await?;

        if self.current_size() + (file_size as usize) >= self.max_size() {
            return Err(Error::InvalidLength {
                context: path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .map(str::to_string)
                    .unwrap_or("path".to_string()),
                current: self.current_size() + file_size as usize,
                minimum: None,
                maximum: Some(self.max_size()),
            });
        }

        let current_directory = match dest_path {
            Some(dest) => self.root_directory().get_last_directory_from_path(&dest)?,
            None => self.current_directory()?,
        };

        if current_directory.get_item(&name).is_ok() {
            return Err(Error::FileExist);
        }

        let ((width, height), exact) = (
            self.config.thumbnail_size(),
            self.config.thumbnail_exact_format(),
        );

        let thumbnail_store = self.thumbnail_store.clone();

        let ticket = thumbnail_store.insert(&path, width, height, exact).await?;

        let constellation_tx = self.constellation_tx.clone();
        let mut export_tx = self.export_tx.clone();

        let progress_stream = async_stream::stream! {
            let mut last_written = 0;

            let mut total_written = 0;
            let mut returned_path = None;

            let mut stream = ipfs.add_unixfs(path);

            while let Some(status) = stream.next().await {
                let name = name.clone();
                match status {
                    UnixfsStatus::CompletedStatus { path, written, total_size } => {
                        returned_path = Some(path);
                        total_written = written;
                        last_written = written;
                        yield Progression::CurrentProgress {
                            name,
                            current: written,
                            total: total_size,
                        };
                    }
                    UnixfsStatus::FailedStatus {
                        written, error, ..
                    } => {
                        last_written = written;
                        let error = error.into();
                        yield Progression::ProgressFailed {
                            name,
                            last_size: Some(last_written),
                            error,
                        };
                        return;
                    }
                    UnixfsStatus::ProgressStatus { written, total_size } => {
                        last_written = written;
                        yield Progression::CurrentProgress {
                            name,
                            current: written,
                            total: total_size,
                        };
                    }
                }
            }
            let ipfs_path = match
                returned_path {
                    Some(path) => path,
                    None => {
                        yield Progression::ProgressFailed {
                            name,
                            last_size: Some(last_written),
                            error: Error::Other,
                        };
                        return;
                    }
                };

            let file = warp::constellation::file::File::new(&name);
            file.set_size(total_written);
            file.set_reference(&format!("{ipfs_path}"));
            file.set_file_type(to_file_type(&name));

            match thumbnail_store.get(ticket).await {
                Ok((extension_type, path, thumbnail)) => {
                    file.set_thumbnail(thumbnail);
                    file.set_thumbnail_format(extension_type.into());
                    file.set_thumbnail_reference(&path.to_string());
                }
                Err(e) => {
                    tracing::error!(error = %e, ticket = %ticket, "Error generating thumbnail");
                }
            }

            if let Err(e) = current_directory.add_item(file) {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: e,
                };
                return;
            }

            let _ = export_tx.try_send(());

            yield Progression::ProgressComplete {
                name: name.to_string(),
                total: Some(total_written),
            };

            constellation_tx.emit(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written)
            }).await;
        };

        Ok(progress_stream.boxed())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn get(&self, name: &str, path: &str) -> Result<ConstellationProgressStream, Error> {
        let ipfs = self.ipfs.clone();

        let path = PathBuf::from(path);
        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let reference = file.reference().ok_or(Error::Other)?.parse::<IpfsPath>()?; //Reference not found
        let fs_tx = self.constellation_tx.clone();
        let name = name.to_string();

        let stream = async_stream::stream! {
            let mut stream = ipfs.get_unixfs(reference, &path);
            while let Some(status) = stream.next().await {
                match status {
                    UnixfsStatus::CompletedStatus { total_size, .. } => {
                        yield Progression::ProgressComplete {
                            name: name.to_string(),
                            total: total_size,
                        };
                    }
                    UnixfsStatus::FailedStatus {
                        written, error, ..
                    } => {
                        yield Progression::ProgressFailed {
                            name: name.to_string(),
                            last_size: Some(written),
                            error: error.into(),
                        };
                        return;
                    }
                    UnixfsStatus::ProgressStatus { written, total_size } => {
                        yield Progression::CurrentProgress {
                            name: name.to_string(),
                            current: written,
                            total: total_size,
                        };
                    }
                }
            }

            fs_tx
                .emit(ConstellationEventKind::Downloaded {
                    filename: file.name(),
                    size: Some(file.size()),
                    location: Some(path),
                })
                .await;

        };

        Ok(stream.boxed())
    }

    fn put_buffer(
        &self,
        name: String,
        buffer: Bytes,
    ) -> Result<BoxFuture<'static, Result<(), Error>>, Error> {
        let ipfs = self.ipfs.clone();
        let thumbnail_store = self.thumbnail_store.clone();
        let tx = self.constellation_tx.clone();
        let mut export_tx = self.export_tx.clone();
        let thumbnail_size = self.config.thumbnail_size();
        let thumbnail_format = self.config.thumbnail_exact_format();

        let (name, dest_path) = split_file_from_path(name)?;

        if self.current_size() + buffer.len() >= self.max_size() {
            return Err(Error::InvalidLength {
                context: "buffer".into(),
                current: self.current_size() + buffer.len(),
                minimum: None,
                maximum: Some(self.max_size()),
            });
        }

        if let Some(max_file_size) = self.config.max_storage_size() {
            if buffer.len() > max_file_size {
                return Err(Error::InvalidLength {
                    context: "buffer".into(),
                    minimum: None,
                    maximum: Some(max_file_size),
                    current: buffer.len(),
                });
            }
        }

        let current_directory = match dest_path {
            Some(dest) => self.root_directory().get_last_directory_from_path(&dest)?,
            None => self.current_directory()?,
        };

        Ok(async move {
            if current_directory.get_item_by_path(&name).is_ok() {
                return Err(Error::FileExist);
            }

            let ((width, height), exact) = (thumbnail_size, thumbnail_format);

            let ticket = thumbnail_store
                .insert_buffer(&name, &buffer, width, height, exact)
                .await;

            let mut total_written = 0;
            let mut returned_path = None;

            let mut stream = ipfs.add_unixfs(buffer);

            while let Some(status) = stream.next().await {
                match status {
                    UnixfsStatus::CompletedStatus { path, written, .. } => {
                        returned_path = Some(path);
                        total_written = written;
                    }
                    UnixfsStatus::FailedStatus { error, .. } => return Err(error.into()),
                    _ => {}
                }
            }

            let ipfs_path = returned_path.ok_or_else(|| anyhow::anyhow!("Cid was never set"))?;

            let file = warp::constellation::file::File::new(&name);
            file.set_size(total_written);
            file.set_reference(&format!("{ipfs_path}"));
            file.set_file_type(to_file_type(&name));

            match thumbnail_store.get(ticket).await {
                Ok((extension_type, path, thumbnail)) => {
                    file.set_thumbnail(thumbnail);
                    file.set_thumbnail_format(extension_type.into());
                    file.set_thumbnail_reference(&path.to_string());
                }
                Err(e) => {
                    tracing::error!(error = %e, ticket = %ticket, "Error generating thumbnail");
                }
            }

            current_directory.add_item(file)?;

            let _ = export_tx.try_send(());

            tx.emit(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written),
            })
            .await;
            Ok(())
        }
        .boxed())
    }

    fn get_buffer(
        &self,
        name: impl Into<String>,
    ) -> Result<BoxFuture<'static, Result<Bytes, Error>>, Error> {
        let name = name.into();
        let ipfs = self.ipfs.clone();
        let current_directory = self.current_directory()?;
        let tx = self.constellation_tx.clone();

        Ok(async move {
            let item = current_directory.get_item_by_path(&name)?;
            let file = item.get_file()?;
            let reference = file.reference().ok_or(Error::Other)?; //Reference not found

            let buffer = ipfs
                .cat_unixfs(reference.parse::<IpfsPath>()?)
                .await
                .map_err(anyhow::Error::new)?;

            tx.emit(ConstellationEventKind::Downloaded {
                filename: file.name(),
                size: Some(file.size()),
                location: None,
            })
            .await;

            Ok(buffer)
        }
        .boxed())
    }

    /// Used to upload file to the filesystem with data from a stream
    fn put_stream(
        &mut self,
        name: &str,
        total_size: Option<usize>,
        stream: BoxStream<'static, std::io::Result<Bytes>>,
    ) -> Result<ConstellationProgressStream, Error> {
        let (name, dest_path) = split_file_from_path(name)?;

        let ipfs = self.ipfs.clone();

        let current_directory = match dest_path {
            Some(dest) => self.root_directory().get_last_directory_from_path(&dest)?,
            None => self.current_directory()?,
        };

        if current_directory.get_item_by_path(&name).is_ok() {
            return Err(Error::FileExist);
        }

        if let Some(total_size) = total_size {
            if total_size + self.current_size() > self.max_size() {
                return Err(Error::InvalidLength {
                    context: "stream".into(),
                    minimum: None,
                    maximum: Some(self.max_size()),
                    current: self.current_size() + total_size,
                });
            }

            if let Some(max_file_size) = self.config.max_storage_size() {
                if total_size > max_file_size {
                    return Err(Error::InvalidLength {
                        context: "stream".into(),
                        minimum: None,
                        maximum: Some(max_file_size),
                        current: total_size,
                    });
                }
            }
        }

        let constellation_tx = self.constellation_tx.clone();
        let mut export_tx = self.export_tx.clone();
        let max_size = self.max_size();
        let max_file_size = self.config.max_file_size();
        let root = self.root_directory();

        let thumbnail_store = self.thumbnail_store.clone();
        let thumbnail_size = self.config.thumbnail_size();
        let thumbnail_format = self.config.thumbnail_exact_format();

        let progress_stream = async_stream::stream! {

            let mut last_written = 0;

            let mut total_written = 0;
            let mut returned_path = None;

            let mut stream = ipfs.add_unixfs(stream);

            while let Some(status) = stream.next().await {
                let n = name.clone();
                match status {
                    UnixfsStatus::CompletedStatus { path, written, .. } => {
                        returned_path = Some(path);
                        total_written = written;
                        last_written = written;
                        yield Progression::CurrentProgress {
                            name: n,
                            current: written,
                            total: total_size,
                        };
                    }
                    UnixfsStatus::FailedStatus {
                        written, error, ..
                    } => {
                        last_written = written;
                        let error = error.into();
                        yield Progression::ProgressFailed {
                            name: n,
                            last_size: Some(last_written),
                            error,
                        };
                        return;
                    }
                    UnixfsStatus::ProgressStatus { written, .. } => {
                        last_written = written;
                        yield Progression::CurrentProgress {
                            name: n,
                            current: written,
                            total: total_size,
                        };
                    }
                }

                if root.size() + last_written > max_size {
                    yield Progression::ProgressFailed {
                        name,
                        last_size: Some(last_written),
                        error: Error::InvalidLength {
                            context: "buffer".into(),
                            current: root.size() + last_written,
                            minimum: None,
                            maximum: Some(max_size),
                        }
                    };
                    return;
                }

                if let Some(max_file_size) = max_file_size {
                    if last_written > max_file_size {
                        yield Progression::ProgressFailed {
                            name,
                            last_size: Some(last_written),
                            error: Error::InvalidLength {
                                context: "buffer".into(),
                                current: last_written,
                                minimum: None,
                                maximum: Some(max_file_size),
                            }
                        };
                        return;
                    }
                }

            }
            let ipfs_path = match
                returned_path {
                    Some(path) => path,
                    None => {
                        yield Progression::ProgressFailed {
                            name,
                            last_size: Some(last_written),
                            error: Error::Other,
                        };
                        return;
                    }
                };

            // NOTE: To prevent the need of "cloning" the main stream, we will get a stream of bytes from rust-ipfs to pass-through to
            //       the thumbnail store.
            let st = ipfs
                .cat_unixfs(ipfs_path.clone())
                .max_length(MAX_THUMBNAIL_STREAM_SIZE)
                .map(|result| result.map_err(std::io::Error::other))
                .boxed();

            let ((width, height), exact) = (thumbnail_size, thumbnail_format);

            let ticket = thumbnail_store.insert_stream(&name, st, width, height, exact, MAX_THUMBNAIL_STREAM_SIZE).await;

            let file = warp::constellation::file::File::new(&name);
            file.set_size(total_written);
            file.set_reference(&format!("{ipfs_path}"));
            file.set_file_type(to_file_type(&name));

            match thumbnail_store.get(ticket).await {
                Ok((extension_type, path, thumbnail)) => {
                    file.set_thumbnail(thumbnail);
                    file.set_thumbnail_format(extension_type.into());
                    file.set_thumbnail_reference(&path.to_string());
                }
                Err(e) => {
                    tracing::error!(error = %e, ticket = %ticket, "Error generating thumbnail");
                }
            }

            if let Err(e) = current_directory.add_item(file) {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: e,
                };
                return;
            }

            let _ = export_tx.try_send(());

            yield Progression::ProgressComplete {
                name: name.to_string(),
                total: Some(total_written),
            };

            constellation_tx.emit(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written)
            }).await;
        };

        Ok(progress_stream.boxed())
    }

    /// Used to download data from the filesystem using a stream
    fn get_stream(
        &self,
        name: &str,
    ) -> Result<BoxStream<'static, Result<Bytes, std::io::Error>>, Error> {
        let ipfs = self.ipfs.clone();

        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let size = file.size();
        let reference = file.reference().ok_or(Error::Other)?; //Reference not found
        let path = reference.parse::<IpfsPath>()?;
        let tx = self.constellation_tx.clone();

        let stream = ipfs
            .cat_unixfs(path)
            .map_err(std::io::Error::other)
            .try_finally(move || async move {
                let _ = tx
                    .emit(ConstellationEventKind::Downloaded {
                        filename: file.name(),
                        size: Some(size),
                        location: None,
                    })
                    .await;
            });

        //TODO: Validate file against the hashed reference
        Ok(stream.boxed())
    }

    /// Used to remove data from the filesystem
    async fn remove(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        let directory = self.current_directory()?;

        let name = name.trim();

        let item = match name {
            "/" => Item::from(&directory),
            path => directory.get_item_by_path(path)?,
        };

        if !recursive
            && item.is_directory()
            && item.size() > 0
            && item
                .directory()
                .map(|s| !s.get_items().is_empty())
                .unwrap_or_default()
        {
            return Err(Error::DirectoryNotEmpty);
        }

        _remove(&self.ipfs, &directory, &item).await?;

        let _ = self.export().await;

        self.constellation_tx
            .emit(ConstellationEventKind::Deleted {
                item_name: name.to_string(),
            })
            .await;

        Ok(())
    }

    async fn rename(&mut self, current: &str, new: &str) -> Result<(), Error> {
        let (current, dest_path) = split_file_from_path(current)?;

        let current_directory = match dest_path {
            Some(dest) => self.root_directory().get_last_directory_from_path(&dest)?,
            None => self.current_directory()?,
        };

        if current_directory.has_item(new) {
            return Err(Error::DuplicateName);
        }

        current_directory.rename_item(&current, new)?;

        self.export().await?;

        self.constellation_tx
            .emit(ConstellationEventKind::Renamed {
                old_item_name: current.to_string(),
                new_item_name: new.to_string(),
            })
            .await;
        Ok(())
    }

    async fn create_directory(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        let directory = self.current_directory()?;

        //Prevent creating recursive/nested directorieis if `recursive` isnt true
        if name.contains('/') && !recursive {
            return Err(Error::InvalidDirectory);
        }

        if directory.has_item(name) || directory.get_item_by_path(name).is_ok() {
            return Err(Error::DirectoryExist);
        }

        directory.add_directory(Directory::new(name))?;

        let _ = self.export().await;

        Ok(())
    }

    fn sync_ref(&mut self, path: &str) -> Result<BoxFuture<'static, Result<(), Error>>, Error> {
        let ipfs = self.ipfs.clone();
        let thumbnail_store = self.thumbnail_store.clone();
        let thumbnail_size = self.config.thumbnail_size();
        let thumbnail_format = self.config.thumbnail_exact_format();

        let directory = self.current_directory()?;
        let file = directory
            .get_item_by_path(path)
            .and_then(|item| item.get_file())?;

        let reference = file.reference().ok_or(Error::FileNotFound)?;

        let mut export_tx = self.export_tx.clone();

        Ok(async move {
            let buffer = ipfs
                .cat_unixfs(reference.parse::<IpfsPath>()?)
                .await
                .map_err(anyhow::Error::from)?;

            let ((width, height), exact) = (thumbnail_size, thumbnail_format);

            // Generate the thumbnail for the file
            let id = thumbnail_store
                .insert_buffer(file.name(), &buffer, width, height, exact)
                .await;

            if let Ok((extension_type, path, thumbnail)) = thumbnail_store.get(id).await {
                file.set_thumbnail(thumbnail);
                file.set_thumbnail_format(extension_type.into());
                file.set_thumbnail_reference(&path.to_string());
            }

            let _ = export_tx.send(()).await;

            Ok(())
        }
        .boxed())
    }
}

fn split_file_from_path(name: impl Into<String>) -> Result<(String, Option<String>), Error> {
    let name = name.into();
    let mut split_path = name.split('/').collect::<VecDeque<_>>();
    if split_path.is_empty() {
        return Err(Error::InvalidLength {
            context: "name".into(),
            current: 0,
            minimum: Some(2),
            maximum: Some(256),
        });
    }

    // get expected filename
    let name = split_path.pop_back().expect("valid name").trim();
    let split_path = Vec::from_iter(split_path);
    if name.len() < 2 || name.len() > 256 {
        return Err(Error::InvalidLength {
            context: "name".into(),
            current: name.len(),
            minimum: Some(2),
            maximum: Some(256),
        });
    }
    let dest_path = (!split_path.is_empty()).then(|| split_path.join("/"));
    Ok((name.to_string(), dest_path))
}

#[async_recursion::async_recursion]
async fn _remove(ipfs: &Ipfs, root: &Directory, item: &Item) -> Result<(), Error> {
    match item {
        Item::File(file) => {
            let reference = file
                .reference()
                .ok_or(Error::ObjectNotFound)?
                .parse::<IpfsPath>()?; //Reference not found

            let cid = reference
                .root()
                .cid()
                .copied()
                .ok_or_else(|| anyhow::anyhow!("Invalid path root"))?;

            if ipfs.is_pinned(cid).await? {
                ipfs.remove_pin(cid).recursive().await?;
            }

            let name = item.name();
            if let Err(e) = root.remove_item(&name) {
                tracing::error!(error = %e, item_name = %name, "unable to remove file");
            }
        }
        Item::Directory(directory) => {
            for item in directory.get_items() {
                let name = item.name();
                let item_type = item.item_type();

                if let Err(e) = _remove(ipfs, directory, &item).await {
                    tracing::error!(error = %e, item_type = %item_type, item_name = %name, "unable to remove item");
                    continue;
                }
                if item.size() == 0 || item.item_type() == ItemType::DirectoryItem {
                    if let Err(e) = directory.remove_item(&name) {
                        tracing::error!(error = %e, item_type = %item_type, item_name = %name, "unable to remove item");
                    }
                }
            }
            if directory.size() == 0 {
                if let Err(e) = root.remove_item(&directory.name()) {
                    tracing::error!(error = %e, item_name = %directory.name(), "unable to remove directory");
                }
            }
        }
    }

    Ok(())
}

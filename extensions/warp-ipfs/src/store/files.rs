use std::{collections::VecDeque, ffi::OsStr, path::PathBuf, sync::Arc};

use chrono::{DateTime, Utc};
use futures::{
    channel::{mpsc, oneshot},
    future::{self, BoxFuture},
    stream::{self, BoxStream, FuturesUnordered},
    FutureExt, SinkExt, StreamExt,
};
use libipld::Cid;
use rust_ipfs::{
    unixfs::{AddOption, UnixfsStatus},
    Ipfs, IpfsPath,
};

use tokio_util::io::ReaderStream;
use tracing::{Instrument, Span};
use warp::{
    constellation::{
        directory::Directory, ConstellationEventKind, ConstellationProgressStream, Progression,
    },
    error::Error,
};

use parking_lot::RwLock;

use crate::{
    config::{self, Config},
    thumbnail::ThumbnailGenerator,
    to_file_type,
};

use super::{ecdh_decrypt, ecdh_encrypt, event_subscription::EventSubscription, get_keypair_did};

#[derive(Clone)]
pub struct FileStore {
    index: Directory,
    path: Arc<RwLock<PathBuf>>,
    config: config::Config,
    tx: mpsc::Sender<FileTaskCommand>,
}

impl FileStore {
    pub async fn new(
        ipfs: Ipfs,
        config: &Config,
        constellation_tx: EventSubscription<ConstellationEventKind>,
        span: Span,
    ) -> Result<Self, Error> {
        let mut index_cid = None;

        if let Some(path) = config.path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }

            if let Ok(cid) = tokio::fs::read(path.join(".index_id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .and_then(|cid_str| {
                    cid_str
                        .parse::<Cid>()
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                })
            {
                index_cid = Some(cid);
            }
        }

        let config = config.clone();

        let index = Directory::new("root");
        let thumbnail_store = ThumbnailGenerator::new(ipfs.clone());

        let (tx, rx) = futures::channel::mpsc::channel(1);
        let (signal_tx, signal_rx) = futures::channel::mpsc::unbounded();

        let mut task = FileTask {
            index,
            path: Arc::default(),
            index_cid,
            thumbnail_store,
            ipfs,
            constellation_tx,
            config,
            signal_tx,
            signal_rx,
            rx,
        };

        if let Err(e) = task.import().await {
            tracing::error!("Error importing index: {e}");
        }

        let mut index = task.index.clone();
        let path = task.path.clone();
        let config = task.config.clone();
        let _span = span.clone();

        let signal = Some(task.signal_tx.clone());
        index.rebuild_paths(&signal);

        tokio::spawn(async move {
            tokio::select! {
                _ = task.run().instrument(_span) => {}
            }
        });

        Ok(FileStore {
            index,
            config,
            path,
            tx,
        })
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
        self.config.max_storage_size.unwrap_or(1024 * 1024 * 1024)
    }

    pub fn set_path(&mut self, path: PathBuf) {
        *self.path.write() = path;
    }

    pub fn get_path(&self) -> PathBuf {
        PathBuf::from(self.path.read().to_string_lossy().replace('\\', "/"))
    }

    pub async fn put(
        &mut self,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<ConstellationProgressStream, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(FileTaskCommand::Put {
                name: name.into(),
                path: path.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get(
        &self,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<ConstellationProgressStream, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
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
        buffer: impl Into<Vec<u8>>,
    ) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(FileTaskCommand::PutBuffer {
                name: name.into(),
                buffer: buffer.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    pub async fn get_buffer(&self, name: impl Into<String>) -> Result<Vec<u8>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .clone()
            .send(FileTaskCommand::GetBuffer {
                name: name.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }

    /// Used to upload file to the filesystem with data from a stream
    pub async fn put_stream(
        &mut self,
        name: impl Into<String>,
        total_size: impl Into<Option<usize>>,
        stream: BoxStream<'static, Vec<u8>>,
    ) -> Result<ConstellationProgressStream, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
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
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
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
            .tx
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
            .tx
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
            .tx
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
            .tx
            .clone()
            .send(FileTaskCommand::SyncRef {
                path: path.into(),
                response: tx,
            })
            .await;
        rx.await.map_err(anyhow::Error::from)?
    }
}

type GetStream = BoxStream<'static, Result<Vec<u8>, Error>>;

enum FileTaskCommand {
    Put {
        name: String,
        path: String,
        response: oneshot::Sender<Result<ConstellationProgressStream, Error>>,
    },
    PutBuffer {
        name: String,
        buffer: Vec<u8>,
        response: oneshot::Sender<Result<(), Error>>,
    },
    PutStream {
        name: String,
        total_size: Option<usize>,
        stream: BoxStream<'static, Vec<u8>>,
        response: oneshot::Sender<Result<ConstellationProgressStream, Error>>,
    },

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
        response: oneshot::Sender<Result<Vec<u8>, Error>>,
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
        response: oneshot::Sender<Result<(), Error>>,
    },
}

enum FutResult {
    Put {
        result: Result<(), Error>,
        response: oneshot::Sender<Result<(), Error>>,
    },
    Get {
        result: Result<Vec<u8>, Error>,
        response: oneshot::Sender<Result<Vec<u8>, Error>>,
    },
    Sync {
        result: Result<(), Error>,
        response: oneshot::Sender<Result<(), Error>>,
    },
}

struct FileTask {
    index: Directory,
    path: Arc<RwLock<PathBuf>>,
    index_cid: Option<Cid>,
    config: config::Config,
    ipfs: Ipfs,
    signal_tx: futures::channel::mpsc::UnboundedSender<()>,
    signal_rx: futures::channel::mpsc::UnboundedReceiver<()>,
    thumbnail_store: ThumbnailGenerator,
    constellation_tx: EventSubscription<ConstellationEventKind>,
    rx: futures::channel::mpsc::Receiver<FileTaskCommand>,
}

impl FileTask {
    async fn run(&mut self) {
        let mut futs: FuturesUnordered<BoxFuture<'static, FutResult>> = FuturesUnordered::new();
        futs.push(future::pending().boxed());
        loop {
            tokio::select! {
                biased;
                Some(command) = self.rx.next() => {
                    match command {
                        FileTaskCommand::Put {
                            name,
                            path,
                            response,
                        } => {
                            _ = response.send(self.put(&name, &path).await);
                        },
                        FileTaskCommand::PutBuffer {
                            name,
                            buffer,
                            response,
                        } => {
                            let fut = match self.put_buffer(name, buffer) {
                                Ok(fut) => fut.then(|result| async move {
                                    FutResult::Put { result, response }
                                }).boxed(),
                                Err(e) => {
                                    _ = response.send(Err(e));
                                    continue;
                                }
                            };

                           futs.push(fut);
                        },
                        FileTaskCommand::PutStream {
                            name,
                            total_size,
                            stream,
                            response,
                        } => {
                            _ = response.send(self.put_stream(&name, total_size, stream));
                        },
                        FileTaskCommand::Get {
                            name,
                            path,
                            response,
                        } => {
                            _ = response.send(self.get(&name, &path));
                        },
                        FileTaskCommand::GetStream { name, response } => {
                            _ = response.send(self.get_stream(&name));
                        },
                        FileTaskCommand::GetBuffer { name, response } => {
                            let fut = match self.get_buffer(name) {
                                Ok(fut) => fut.then(|result| async move {
                                    FutResult::Get { result, response }
                                }).boxed(),
                                Err(e) => {
                                    _ = response.send(Err(e));
                                    continue;
                                }
                            };

                           futs.push(fut);
                        },
                        FileTaskCommand::Remove {
                            name,
                            recursive,
                            response,
                        } => {
                            _ = response.send(self.remove(&name, recursive).await);
                        },
                        FileTaskCommand::Rename {
                            current,
                            new,
                            response,
                        } => {
                            _ = response.send(self.rename(&current, &new).await);
                        },
                        FileTaskCommand::CreateDirectory {
                            name,
                            recursive,
                            response,
                        } => {
                            _ = response.send(self.create_directory(&name, recursive));
                        },
                        FileTaskCommand::SyncRef { path, response } => {
                            let fut = match self.sync_ref(&path) {
                                Ok(fut) => fut.then(|result| async move {
                                    FutResult::Sync { result, response }
                                }).boxed(),
                                Err(e) => {
                                    _ = response.send(Err(e));
                                    continue;
                                }
                            };

                           futs.push(fut);
                        },
                    }
                },
                Some(fut) = futs.next() => {
                    match fut {
                        FutResult::Put { result, response } => {
                            _ = response.send(result);
                        },
                        FutResult::Get { result, response } => {
                            _ = response.send(result);
                        },
                        FutResult::Sync { result, response } => {
                            _ = response.send(result);
                        }
                    }
                },
                Some(_) = self.signal_rx.next() => {
                    if let Err(_e) = self.export().await {
                        tracing::error!("Error exporting index: {_e}");
                    }
                }
            }
        }
    }

    pub async fn import(&self) -> Result<(), Error> {
        if self.config.path.is_none() {
            return Ok(());
        }

        tracing::info!("Importing index");
        let cid = self.index_cid.ok_or(Error::Other)?;

        tracing::info!(cid = %cid, "Index located");

        let data = self
            .ipfs
            .unixfs()
            .cat(cid, None, &[], true, None)
            .await
            .map_err(anyhow::Error::from)?;

        let key = self.ipfs.keypair().and_then(get_keypair_did)?;

        let index_bytes = ecdh_decrypt(&key, None, data)?;

        let directory_index: Directory = serde_json::from_slice(&index_bytes)?;
        self.index.set_items(directory_index.get_items());

        tracing::info!(cid = %cid, "Index imported");

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn export(&mut self) -> Result<(), Error> {
        tracing::trace!("Exporting index");

        let mut index = self.index.clone();
        let signal = Some(self.signal_tx.clone());

        index.rebuild_paths(&signal);

        let index = serde_json::to_string(&self.index)?;

        let key = self.ipfs.keypair().and_then(get_keypair_did)?;

        //TODO: Disable encryption of the index but instead store it all as a dag object.
        //      possibly even building an internal graph of the index instead
        let data = ecdh_encrypt(&key, None, index.as_bytes())?;

        let data_stream = stream::once(async move { Ok::<_, std::io::Error>(data) }).boxed();

        let ipfs_path = self
            .ipfs
            .unixfs()
            .add(
                data_stream,
                Some(AddOption {
                    pin: true,
                    ..Default::default()
                }),
            )
            .await?;

        tracing::trace!(path = %ipfs_path, "Index exported");

        let cid = ipfs_path
            .root()
            .cid()
            .ok_or(Error::OtherWithContext("unable to get cid".into()))?;

        let last_cid = self.index_cid.replace(*cid);

        if let Some(path) = self.config.path.as_ref() {
            if let Err(_e) = tokio::fs::write(path.join(".index_id"), cid.to_string()).await {
                tracing::error!(cid = %cid, "Error writing index: {_e}");
            }
        }

        if let Some(last_cid) = last_cid {
            if *cid != last_cid {
                if self.ipfs.is_pinned(&last_cid).await? {
                    tracing::trace!(cid = %last_cid, "Unpinning block");
                    self.ipfs.remove_pin(&last_cid).recursive().await?;
                    tracing::trace!(cid = %last_cid, "Block unpinned");
                }

                tracing::trace!(cid = %last_cid, "Removing block");
                let b = self.ipfs.remove_block(last_cid, true).await?;
                tracing::trace!(cid = %last_cid, amount_removed = b.len(), "Blocks removed");
            }
        }

        tracing::trace!(cid = %cid, "Index exported");
        Ok(())
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

    fn root_directory(&self) -> Directory {
        self.index.clone()
    }

    /// Current size of the file system
    fn current_size(&self) -> usize {
        self.root_directory().size()
    }

    fn max_size(&self) -> usize {
        self.config.max_storage_size.unwrap_or(1024 * 1024 * 1024)
    }

    fn get_path(&self) -> PathBuf {
        PathBuf::from(self.path.read().to_string_lossy().replace('\\', "/"))
    }

    async fn put(&mut self, name: &str, path: &str) -> Result<ConstellationProgressStream, Error> {
        let (name, dest_path) = split_file_from_path(name)?;

        let ipfs = self.ipfs.clone();

        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(Error::FileNotFound);
        }

        let file_size = tokio::fs::metadata(&path).await?.len();

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
            self.config.thumbnail_size,
            self.config.thumbnail_exact_format,
        );

        let thumbnail_store = self.thumbnail_store.clone();

        let ticket = thumbnail_store.insert(&path, width, height, exact).await?;

        let background = self.config.thumbnail_task;

        let name = name.to_string();
        let constellation_tx = self.constellation_tx.clone();

        let progress_stream = async_stream::stream! {
            let _current_guard = current_directory.signal_guard();
            let mut last_written = 0;

            let mut total_written = 0;
            let mut returned_path = None;

            let mut stream = ipfs.unixfs().add(path.clone(), Some(AddOption { pin: true, ..Default::default() }));

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
                        yield Progression::ProgressFailed {
                            name,
                            last_size: Some(last_written),
                            error: error.map(|e| e.to_string()),
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
                            error: Some("IpfsPath not set".into()),
                        };
                        return;
                    }
                };

            let file = warp::constellation::file::File::new(&name);
            file.set_size(total_written);
            file.set_reference(&format!("{ipfs_path}"));
            file.set_file_type(to_file_type(&name));

            if let Err(e) = current_directory.add_item(file.clone()) {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: Some(e.to_string()),
                };
                return;
            }

            let task = {
                let store = thumbnail_store.clone();
                let file = file.clone();
                async move {
                    match store.get(ticket).await {
                        Ok((extension_type, path, thumbnail)) => {
                            file.set_thumbnail(&thumbnail);
                            file.set_thumbnail_format(extension_type.into());
                            file.set_thumbnail_reference(&path.to_string());
                        }
                        Err(e) => {
                            tracing::error!(error = %e, ticket = %ticket, "Error generating thumbnail");
                        }
                    }
                }
            };

            if !background {
                task.await;
            } else {
                tokio::spawn(task);
            }

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
                            error: error.map(|e| e.to_string()),
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
        buffer: Vec<u8>,
    ) -> Result<BoxFuture<'static, Result<(), Error>>, Error> {
        let ipfs = self.ipfs.clone();
        let thumbnail_store = self.thumbnail_store.clone();
        let tx = self.constellation_tx.clone();
        let thumbnail_size = self.config.thumbnail_size;
        let thumbnail_format = self.config.thumbnail_exact_format;

        let (name, dest_path) = split_file_from_path(name)?;

        if self.current_size() + buffer.len() >= self.max_size() {
            return Err(Error::InvalidLength {
                context: "buffer".into(),
                current: self.current_size() + buffer.len(),
                minimum: None,
                maximum: Some(self.max_size()),
            });
        }

        let current_directory = match dest_path {
            Some(dest) => self.root_directory().get_last_directory_from_path(&dest)?,
            None => self.current_directory()?,
        };

        Ok(async move {
            let _current_guard = current_directory.signal_guard();

            if current_directory.get_item_by_path(&name).is_ok() {
                return Err(Error::FileExist);
            }

            let ((width, height), exact) = (thumbnail_size, thumbnail_format);

            let ticket = thumbnail_store
                .insert_buffer(&name, &buffer, width, height, exact)
                .await;

            let reader = ReaderStream::new(std::io::Cursor::new(&buffer))
                .map(|result| result.map(|x| x.into()))
                .boxed();

            let mut total_written = 0;
            let mut returned_path = None;

            let mut stream = ipfs.unixfs().add(
                reader,
                Some(AddOption {
                    pin: true,
                    ..Default::default()
                }),
            );

            while let Some(status) = stream.next().await {
                match status {
                    UnixfsStatus::CompletedStatus { path, written, .. } => {
                        returned_path = Some(path);
                        total_written = written;
                    }
                    UnixfsStatus::FailedStatus { error, .. } => {
                        return Err(error.map(Error::Any).unwrap_or(Error::Other))
                    }
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
                    file.set_thumbnail(&thumbnail);
                    file.set_thumbnail_format(extension_type.into());
                    file.set_thumbnail_reference(&path.to_string());
                }
                Err(e) => {
                    tracing::error!(error = %e, ticket = %ticket, "Error generating thumbnail");
                }
            }

            current_directory.add_item(file)?;

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
    ) -> Result<BoxFuture<'static, Result<Vec<u8>, Error>>, Error> {
        let name = name.into();
        let ipfs = self.ipfs.clone();
        let current_directory = self.current_directory()?;
        let tx = self.constellation_tx.clone();

        Ok(async move {
            let item = current_directory.get_item_by_path(&name)?;
            let file = item.get_file()?;
            let reference = file.reference().ok_or(Error::Other)?; //Reference not found

            let buffer = ipfs
                .cat_unixfs(reference.parse::<IpfsPath>()?, None)
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
        stream: BoxStream<'static, Vec<u8>>,
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

        let name = name.to_string();
        let stream = stream.map(Ok::<_, std::io::Error>).boxed();
        let constellation_tx = self.constellation_tx.clone();
        let max_size = self.max_size();
        let root = self.root_directory();

        let progress_stream = async_stream::stream! {

            let mut last_written = 0;

            let mut total_written = 0;
            let mut returned_path = None;

            let mut stream = ipfs.unixfs().add(stream, Some(AddOption { pin: true, ..Default::default() }));

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
                        yield Progression::ProgressFailed {
                            name: n,
                            last_size: Some(last_written),
                            error: error.map(|e| e.to_string()),
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

                if root.size() + last_written >= max_size {
                    yield Progression::ProgressFailed {
                        name,
                        last_size: Some(last_written),
                        error: Some(Error::InvalidLength {
                            context: "buffer".into(),
                            current: root.size() + last_written,
                            minimum: None,
                            maximum: Some(max_size),
                        }).map(|e| e.to_string())
                    };
                    return;
                }

            }
            let ipfs_path = match
                returned_path {
                    Some(path) => path,
                    None => {
                        yield Progression::ProgressFailed {
                            name,
                            last_size: Some(last_written),
                            error: Some("IpfsPath not set".into()),
                        };
                        return;
                    }
                };

            let _current_guard = current_directory.signal_guard();

            let file = warp::constellation::file::File::new(&name);
            file.set_size(total_written);
            file.set_reference(&format!("{ipfs_path}"));
            file.set_file_type(to_file_type(&name));

            if let Err(e) = current_directory.add_item(file) {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: Some(e.to_string()),
                };
                return;
            }

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
    fn get_stream(&self, name: &str) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        let ipfs = self.ipfs.clone();

        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let size = file.size();
        let reference = file.reference().ok_or(Error::Other)?; //Reference not found

        let tx = self.constellation_tx.clone();

        let stream = async_stream::stream! {
            let cat_stream = ipfs
                .cat_unixfs(reference.parse::<IpfsPath>()?, None);

            for await data in cat_stream {
                match data {
                    Ok(data) => yield Ok(data),
                    Err(e) => yield Err(Error::from(anyhow::anyhow!("{e}"))),
                }
            }

            let _ = tx.emit(ConstellationEventKind::Downloaded { filename: file.name(), size: Some(size), location: None }).await;
        };

        //TODO: Validate file against the hashed reference
        Ok(stream.boxed())
    }

    /// Used to remove data from the filesystem
    async fn remove(&mut self, name: &str, _: bool) -> Result<(), Error> {
        let ipfs = self.ipfs.clone();
        //TODO: Recursively delete directory but for now only support deleting a file
        let directory = self.current_directory()?;
        let _g = directory.signal_guard();

        let item = directory.get_item_by_path(name)?;

        let file = item.get_file()?;
        let reference = file
            .reference()
            .ok_or(Error::ObjectNotFound)?
            .parse::<IpfsPath>()?; //Reference not found

        let cid = reference
            .root()
            .cid()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Invalid path root"))?;

        if ipfs.is_pinned(&cid).await? {
            ipfs.remove_pin(&cid).recursive().await?;
        }

        directory.remove_item(&item.name())?;

        let blocks = ipfs.remove_block(cid, true).await.unwrap_or_default();
        tracing::info!(blocks = blocks.len(), "blocks removed");

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

        let _g = current_directory.signal_guard();
        if current_directory.has_item(new) {
            return Err(Error::DuplicateName);
        }

        current_directory.rename_item(&current, new)?;

        self.constellation_tx
            .emit(ConstellationEventKind::Renamed {
                old_item_name: current.to_string(),
                new_item_name: new.to_string(),
            })
            .await;
        Ok(())
    }

    fn create_directory(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        let directory = self.current_directory()?;
        let _g = directory.signal_guard();

        //Prevent creating recursive/nested directorieis if `recursive` isnt true
        if name.contains('/') && !recursive {
            return Err(Error::InvalidDirectory);
        }

        if directory.has_item(name) || directory.get_item_by_path(name).is_ok() {
            return Err(Error::DirectoryExist);
        }

        directory.add_directory(Directory::new(name))?;

        Ok(())
    }

    fn sync_ref(&mut self, path: &str) -> Result<BoxFuture<'static, Result<(), Error>>, Error> {
        let ipfs = self.ipfs.clone();
        let thumbnail_store = self.thumbnail_store.clone();
        let thumbnail_size = self.config.thumbnail_size;
        let thumbnail_format = self.config.thumbnail_exact_format;

        let directory = self.current_directory()?;
        let file = directory
            .get_item_by_path(path)
            .and_then(|item| item.get_file())?;

        let reference = file.reference().ok_or(Error::FileNotFound)?;

        Ok(async move {
            let buffer = ipfs
                .cat_unixfs(reference.parse::<IpfsPath>()?, None)
                .await
                .map_err(anyhow::Error::from)?;

            let ((width, height), exact) = (thumbnail_size, thumbnail_format);

            // Generate the thumbnail for the file
            let id = thumbnail_store
                .insert_buffer(file.name(), &buffer, width, height, exact)
                .await;

            if let Ok((extension_type, path, thumbnail)) = thumbnail_store.get(id).await {
                file.set_thumbnail(&thumbnail);
                file.set_thumbnail_format(extension_type.into());
                file.set_thumbnail_reference(&path.to_string());
            }

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

use std::{ffi::OsStr, path::PathBuf, sync::Arc, task::Poll};

use chrono::{DateTime, Utc};
use futures::{stream::BoxStream, StreamExt};
use libipld::Cid;
use rust_ipfs::{
    unixfs::{AddOption, UnixfsStatus},
    Ipfs, IpfsPath,
};

use tokio::sync::Notify;
use tokio_util::io::ReaderStream;
use tracing::Span;
use warp::{
    constellation::{
        directory::Directory, ConstellationEventKind, ConstellationProgressStream, Progression,
    },
    error::Error,
    sync::RwLock,
};

use crate::{
    config::{self, Config},
    thumbnail::ThumbnailGenerator,
    to_file_type,
};

use super::{
    document::root::RootDocumentMap, ecdh_decrypt, event_subscription::EventSubscription,
    get_keypair_did,
};

#[derive(Clone)]
#[allow(dead_code)]
pub struct FileStore {
    index: Directory,
    path: Arc<RwLock<PathBuf>>,
    modified: DateTime<Utc>,
    thumbnail_store: ThumbnailGenerator,
    root: RootDocumentMap,

    ipfs: Ipfs,
    constellation_tx: EventSubscription<ConstellationEventKind>,

    signal: futures::channel::mpsc::UnboundedSender<()>,

    config: config::Config,

    span: Span,

    signal_guard: Arc<Notify>,
}

impl FileStore {
    pub async fn new(
        ipfs: Ipfs,
        root: RootDocumentMap,
        config: &Config,
        constellation_tx: EventSubscription<ConstellationEventKind>,
        span: Span,
    ) -> Result<Self, Error> {
        if let Some(path) = config.path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }
        }

        let config = config.clone();

        let index = Directory::new("root");
        let path = Arc::default();
        let modified = Utc::now();
        let thumbnail_store = ThumbnailGenerator::new(ipfs.clone());

        let (tx, mut rx) = futures::channel::mpsc::unbounded();

        let mut store = FileStore {
            index,
            path,
            modified,
            root,
            thumbnail_store,
            ipfs,
            constellation_tx,
            config,
            signal: tx,
            signal_guard: Arc::default(),
            span,
        };

        if let Some(p) = store.config.path.as_ref() {
            let index_file = p.join("index_id");
            if index_file.is_file() {
                if let Err(e) = store.import().await {
                    tracing::error!("Error importing index: {e}");
                }
            }
        }

        if let Err(_e) = store.import_v0().await {}

        let signal = Some(store.signal.clone());
        store.index.rebuild_paths(&signal);

        tokio::spawn({
            let fs = store.clone();
            async move {
                let mut stream_ctx = futures::stream::poll_fn(|cx| {
                    let mut signaled = false;

                    while let Poll::Ready(Some(_)) = rx.poll_next_unpin(cx) {
                        signaled = true;
                    }

                    if signaled {
                        Poll::Ready(Some(()))
                    } else {
                        Poll::Pending
                    }
                });

                while stream_ctx.next().await.is_some() {
                    if let Err(_e) = fs.export().await {
                        tracing::error!("Error exporting index: {_e}");
                    }
                    fs.signal_guard.notify_waiters();
                }
            }
        });

        Ok(store)
    }

    async fn import(&self) -> Result<(), Error> {
        if self.config.path.is_none() {
            return Ok(());
        }

        let path = match self.config.path.as_ref() {
            Some(path) => path,
            None => return Ok(()),
        };

        tracing::info!("Importing index");

        if let Ok(cid) = tokio::fs::read(path.join(".index_id"))
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .and_then(|cid_str| {
                cid_str
                    .parse::<Cid>()
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })
        {
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

            _ = self.export().await;

            if self.ipfs.is_pinned(&cid).await? {
                self.ipfs.remove_pin(&cid).recursive().await?;
            }

            self.ipfs.remove_block(cid, true).await?;
            _ = tokio::fs::remove_file(".index_id").await;
        }

        Ok(())
    }

    async fn import_v0(&self) -> Result<(), Error> {
        let index = self.root.get_root_index().await?;
        self.index.set_items(index.get_items());
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn export(&self) -> Result<(), Error> {
        tracing::trace!("Exporting index");

        let mut index = self.index.clone();
        let signal = Some(self.signal.clone());

        index.rebuild_paths(&signal);

        self.root.set_root_index(index).await?;

        tracing::trace!("Index exported");
        Ok(())
    }
}

impl FileStore {
    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
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

    pub async fn put(
        &mut self,
        name: &str,
        path: &str,
    ) -> Result<ConstellationProgressStream, Error> {
        let name = name.trim();
        if name.len() < 2 || name.len() > 256 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name.len(),
                minimum: Some(2),
                maximum: Some(256),
            });
        }

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

        let current_directory = self.current_directory()?;

        if current_directory.get_item_by_path(name).is_ok() {
            return Err(Error::FileExist);
        }

        let ((width, height), exact) = (
            self.config.thumbnail_size,
            self.config.thumbnail_exact_format,
        );

        let ticket = self
            .thumbnail_store
            .insert(&path, width, height, exact)
            .await?;

        let background = self.config.thumbnail_task;

        let name = name.to_string();
        let fs = self.clone();

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

            if let Ok(Err(_e)) = tokio::task::spawn_blocking({
                let f = file.clone();
                move || f.hash_mut().hash_from_file(&path)
            }).await {}

            if let Err(e) = current_directory.add_item(file.clone()) {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: Some(e.to_string()),
                };
                return;
            }

            let task = {
                let fs = fs.clone();
                let store = fs.thumbnail_store.clone();
                let file = file.clone();
                async move {
                    match store.get(ticket).await {
                        Ok((extension_type, path, thumbnail)) => {
                            file.set_thumbnail(&thumbnail);
                            file.set_thumbnail_format(extension_type.into());
                            file.set_thumbnail_reference(&path.to_string());
                        }
                        Err(_e) => {}
                    }
                }
            };

            if !background {
                task.await;
            } else {
                tokio::spawn(task);
            }

            drop(_current_guard);
            fs.signal_guard.notified().await;

            yield Progression::ProgressComplete {
                name: name.to_string(),
                total: Some(total_written),
            };

            fs.constellation_tx.emit(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written)
            }).await;
        };

        Ok(ConstellationProgressStream(progress_stream.boxed()))
    }

    pub async fn get(&self, name: &str, path: &str) -> Result<(), Error> {
        let ipfs = self.ipfs.clone();

        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let reference = file.reference().ok_or(Error::Other)?; //Reference not found

        let mut stream = ipfs.get_unixfs(reference.parse::<IpfsPath>()?, path);

        while let Some(status) = stream.next().await {
            if let UnixfsStatus::FailedStatus { error, .. } = status {
                return Err(error.map(Error::Any).unwrap_or(Error::Other));
            }
        }

        //TODO: Validate file against the hashed reference
        self.constellation_tx
            .emit(ConstellationEventKind::Downloaded {
                filename: file.name(),
                size: Some(file.size()),
                location: Some(PathBuf::from(path)),
            })
            .await;

        Ok(())
    }

    pub async fn put_buffer(&mut self, name: &str, buffer: &[u8]) -> Result<(), Error> {
        let name = name.trim();
        if name.len() < 2 || name.len() > 256 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name.len(),
                minimum: Some(2),
                maximum: Some(256),
            });
        }

        if self.current_size() + buffer.len() >= self.max_size() {
            return Err(Error::InvalidLength {
                context: "buffer".into(),
                current: self.current_size() + buffer.len(),
                minimum: None,
                maximum: Some(self.max_size()),
            });
        }

        let ipfs = self.ipfs.clone();

        let current_directory = self.current_directory()?;

        let _current_guard = current_directory.signal_guard();

        if current_directory.get_item_by_path(name).is_ok() {
            return Err(Error::FileExist);
        }

        let ((width, height), exact) = (
            self.config.thumbnail_size,
            self.config.thumbnail_exact_format,
        );

        let ticket = self
            .thumbnail_store
            .insert_buffer(&name, buffer, width, height, exact)
            .await;

        let reader = ReaderStream::new(std::io::Cursor::new(buffer))
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

        let file = warp::constellation::file::File::new(name);
        file.set_size(total_written);
        file.set_reference(&format!("{ipfs_path}"));
        file.set_file_type(to_file_type(name));
        file.hash_mut().hash_from_slice(buffer)?;

        match self.thumbnail_store.get(ticket).await {
            Ok((extension_type, path, thumbnail)) => {
                file.set_thumbnail(&thumbnail);
                file.set_thumbnail_format(extension_type.into());
                file.set_thumbnail_reference(&path.to_string());
            }
            Err(_e) => {}
        }

        current_directory.add_item(file)?;

        drop(_current_guard);
        self.signal_guard.notified().await;

        self.constellation_tx
            .emit(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written),
            })
            .await;
        Ok(())
    }

    pub async fn get_buffer(&self, name: &str) -> Result<Vec<u8>, Error> {
        let ipfs = self.ipfs.clone();

        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let reference = file.reference().ok_or(Error::Other)?; //Reference not found

        let buffer = ipfs
            .cat_unixfs(reference.parse::<IpfsPath>()?, None)
            .await
            .map_err(anyhow::Error::new)?;

        self.constellation_tx
            .emit(ConstellationEventKind::Downloaded {
                filename: file.name(),
                size: Some(file.size()),
                location: None,
            })
            .await;

        Ok(buffer)
    }

    /// Used to upload file to the filesystem with data from a stream
    pub async fn put_stream(
        &mut self,
        name: &str,
        total_size: Option<usize>,
        stream: BoxStream<'static, Vec<u8>>,
    ) -> Result<ConstellationProgressStream, Error> {
        let name = name.trim();
        if name.len() < 2 || name.len() > 256 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name.len(),
                minimum: Some(2),
                maximum: Some(256),
            });
        }

        let ipfs = self.ipfs.clone();

        let current_directory = self.current_directory()?;

        if current_directory.get_item_by_path(name).is_ok() {
            return Err(Error::FileExist);
        }

        let fs = self.clone();
        let name = name.to_string();
        let stream = stream.map(Ok::<_, std::io::Error>).boxed();

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

                if fs.current_size() + last_written >= fs.max_size() {
                    yield Progression::ProgressFailed {
                        name,
                        last_size: Some(last_written),
                        error: Some(Error::InvalidLength {
                            context: "buffer".into(),
                            current: fs.current_size() + last_written,
                            minimum: None,
                            maximum: Some(fs.max_size()),
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
            // file.hash_mut().hash_from_slice(buffer)?;
            if let Err(e) = current_directory.add_item(file) {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: Some(e.to_string()),
                };
                return;
            }

            drop(_current_guard);
            fs.signal_guard.notified().await;

            yield Progression::ProgressComplete {
                name: name.to_string(),
                total: Some(total_written),
            };

            fs.constellation_tx.emit(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written)
            }).await;
        };

        Ok(ConstellationProgressStream(progress_stream.boxed()))
    }

    /// Used to download data from the filesystem using a stream
    pub async fn get_stream(
        &self,
        name: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
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
    pub async fn remove(&mut self, name: &str, _: bool) -> Result<(), Error> {
        let ipfs = self.ipfs.clone();
        //TODO: Recursively delete directory but for now only support deleting a file
        let directory = self.current_directory()?;

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

        let _g = directory.signal_guard();

        directory.remove_item(&item.name())?;

        drop(_g);
        self.signal_guard.notified().await;

        let blocks = ipfs.remove_block(cid, true).await.unwrap_or_default();
        tracing::info!(blocks = blocks.len(), "blocks removed");

        self.constellation_tx
            .emit(ConstellationEventKind::Deleted {
                item_name: name.to_string(),
            })
            .await;

        Ok(())
    }

    pub async fn rename(&mut self, current: &str, new: &str) -> Result<(), Error> {
        //Note: This will only support renaming the file or directory in the index
        let directory = self.current_directory()?;

        if directory.get_item_by_path(new).is_ok() {
            return Err(Error::DuplicateName);
        }

        let _g = directory.signal_guard();
        directory.rename_item(current, new)?;

        drop(_g);
        self.signal_guard.notified().await;

        self.constellation_tx
            .emit(ConstellationEventKind::Renamed {
                old_item_name: current.to_string(),
                new_item_name: new.to_string(),
            })
            .await;
        Ok(())
    }

    pub async fn create_directory(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        let directory = self.current_directory()?;

        //Prevent creating recursive/nested directorieis if `recursive` isnt true
        if name.contains('/') && !recursive {
            return Err(Error::InvalidDirectory);
        }

        if directory.has_item(name) || directory.get_item_by_path(name).is_ok() {
            return Err(Error::DirectoryExist);
        }
        let _g = directory.signal_guard();
        directory.add_directory(Directory::new(name))?;

        drop(_g);
        self.signal_guard.notified().await;

        Ok(())
    }

    pub async fn sync_ref(&mut self, path: &str) -> Result<(), Error> {
        let ipfs = self.ipfs.clone();

        let directory = self.current_directory()?;
        let file = directory
            .get_item_by_path(path)
            .and_then(|item| item.get_file())?;

        let reference = file.reference().ok_or(Error::FileNotFound)?;

        let buffer = ipfs
            .cat_unixfs(reference.parse::<IpfsPath>()?, None)
            .await
            .map_err(anyhow::Error::from)?;

        let ((width, height), exact) = (
            self.config.thumbnail_size,
            self.config.thumbnail_exact_format,
        );

        // Generate the thumbnail for the file
        let id = self
            .thumbnail_store
            .insert_buffer(file.name(), &buffer, width, height, exact)
            .await;

        if let Ok((extension_type, path, thumbnail)) = self.thumbnail_store.get(id).await {
            file.set_thumbnail(&thumbnail);
            file.set_thumbnail_format(extension_type.into());
            file.set_thumbnail_reference(&path.to_string());
        }

        Ok(())
    }

    pub fn set_path(&mut self, path: PathBuf) {
        *self.path.write() = path;
    }

    pub fn get_path(&self) -> PathBuf {
        PathBuf::from(self.path.read().to_string_lossy().replace('\\', "/"))
    }
}

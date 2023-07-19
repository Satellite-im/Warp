pub mod config;
pub mod thumbnail;
pub(crate) mod utils;

use config::FsIpfsConfig;
use futures::stream::{self, BoxStream};
use futures::{pin_mut, StreamExt};
use ipfs::unixfs::{AddOpt, AddOption, UnixfsStatus};
use libipld::Cid;
use rust_ipfs as ipfs;
use std::any::Any;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use thumbnail::ThumbnailGenerator;
use tokio::sync::broadcast;
use tokio_util::io::ReaderStream;
use utils::ExtensionType;
use warp::constellation::file::FileType;
use warp::constellation::{
    ConstellationDataType, ConstellationEvent, ConstellationEventKind, ConstellationEventStream,
    ConstellationProgressStream, Progression,
};
use warp::crypto::cipher::Cipher;
use warp::crypto::did_key::{Generate, ECDH};
use warp::crypto::zeroize::Zeroizing;
use warp::crypto::{DIDKey, Ed25519KeyPair, KeyMaterial, DID};
use warp::logging::tracing::{debug, error};
use warp::multipass::MultiPass;
use warp::sync::{Arc, RwLock};

use warp::module::Module;

use chrono::{DateTime, Utc};
use ipfs::{Ipfs, IpfsPath, Keypair};

use warp::constellation::{directory::Directory, Constellation};
use warp::error::Error;
use warp::{Extension, ExtensionEventKind, SingleHandle};

type Result<T> = std::result::Result<T, Error>;

#[allow(clippy::type_complexity)]
#[derive(Clone)]
pub struct IpfsFileSystem {
    index: Directory,
    path: Arc<RwLock<PathBuf>>,
    modified: DateTime<Utc>,
    config: Option<FsIpfsConfig>,
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    keypair: Arc<RwLock<Option<Arc<DID>>>>,
    index_cid: Arc<RwLock<Option<Cid>>>,
    initialized: Arc<AtomicBool>,
    account: Box<dyn MultiPass>,
    broadcast: tokio::sync::broadcast::Sender<ConstellationEventKind>,
    ready_tx: tokio::sync::broadcast::Sender<ExtensionEventKind>,
    thumbnail_store: ThumbnailGenerator,
}

impl IpfsFileSystem {
    pub async fn new(
        account: Box<dyn MultiPass>,
        config: Option<FsIpfsConfig>,
    ) -> anyhow::Result<Self> {
        let (tx, _) = tokio::sync::broadcast::channel(1024);
        let (ready_tx, _) = tokio::sync::broadcast::channel(1024);
        let filesystem = IpfsFileSystem {
            index: Directory::new("root"),
            path: Arc::new(Default::default()),
            modified: Utc::now(),
            config,
            index_cid: Default::default(),
            initialized: Arc::default(),
            account,
            keypair: Default::default(),
            ipfs: Default::default(),
            broadcast: tx,
            thumbnail_store: ThumbnailGenerator::default(),
            ready_tx,
        };

        tokio::spawn({
            let mut filesystem = filesystem.clone();
            let account = filesystem.account.clone();
            async move {
                if !account.is_ready() {
                    let mut stream = match account.extension_subscribe() {
                        Ok(stream) => stream,
                        Err(e) => {
                            error!("Error while getting extensions stream: {e}");
                            return;
                        }
                    };

                    while let Some(event) = stream.next().await {
                        if matches!(event, ExtensionEventKind::Ready) {
                            break;
                        }
                    }
                }
                if let Err(e) = filesystem.initialize().await {
                    error!("Error initializing filesystem: {e}");
                }
            }
        });

        Ok(filesystem)
    }

    async fn initialize(&mut self) -> Result<()> {
        debug!("Initializing or fetch ipfs");

        let account = self.account.clone();

        let ipfs_handle = match account.handle() {
            Ok(handle) if handle.is::<Ipfs>() => handle.downcast_ref::<Ipfs>().cloned(),
            _ => None,
        };

        let ipfs = ipfs_handle.ok_or(Error::ConstellationExtensionUnavailable)?;
        let ipfs_keypair = ipfs.keypair()?;
        *self.keypair.write() = Some(Arc::new(get_keypair_did(ipfs_keypair)?));

        *self.ipfs.write() = Some(ipfs);

        if let Err(_e) = self.import_index().await {
            error!("Error loading index: {_e}");
        }

        tokio::spawn({
            let fs = self.clone();
            async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    if let Err(_e) = fs.export_index().await {
                        error!("Error exporting index: {_e}");
                    }
                }
            }
        });

        self.initialized.store(true, Ordering::SeqCst);

        let _ = self.ready_tx.send(ExtensionEventKind::Ready);
        Ok(())
    }

    pub async fn export_index(&self) -> Result<()> {
        let ipfs = self.ipfs()?;
        let index = self.export(ConstellationDataType::Json)?;

        let key = self.keypair()?;

        let data = ecdh_encrypt(&key, None, index.as_bytes())?;

        let data_stream = stream::once(async move { Ok::<_, std::io::Error>(data) }).boxed();

        let mut stream = ipfs
            .unixfs()
            .add(
                AddOpt::Stream(data_stream),
                Some(AddOption {
                    pin: true,
                    ..Default::default()
                }),
            )
            .await?;

        let mut ipfs_path = None;

        while let Some(status) = stream.next().await {
            if let UnixfsStatus::CompletedStatus { path, .. } = status {
                ipfs_path = Some(path);
            }
        }

        let path = ipfs_path.ok_or(Error::OtherWithContext("unable to get cid".into()))?;

        let cid = path
            .root()
            .cid()
            .ok_or(Error::OtherWithContext("unable to get cid".into()))?;

        let last_cid = { *self.index_cid.read() };

        *self.index_cid.write() = Some(*cid);

        if let Some(last_cid) = last_cid {
            if *cid != last_cid {
                let mut pinned_blocks: HashSet<_> = HashSet::from_iter(
                    ipfs.list_pins(None)
                        .await
                        .filter_map(|r| async move {
                            match r {
                                Ok(v) => Some(v.0),
                                Err(_) => None,
                            }
                        })
                        .collect::<Vec<_>>()
                        .await,
                );

                if ipfs.is_pinned(&last_cid).await? {
                    ipfs.remove_pin(&last_cid, true).await?;
                }

                let new_pinned_blocks: HashSet<_> = HashSet::from_iter(
                    ipfs.list_pins(None)
                        .await
                        .filter_map(|r| async move {
                            match r {
                                Ok(v) => Some(v.0),
                                Err(_) => None,
                            }
                        })
                        .collect::<Vec<_>>()
                        .await,
                );

                for s_cid in new_pinned_blocks.iter() {
                    pinned_blocks.remove(s_cid);
                }

                for cid in pinned_blocks {
                    ipfs.remove_block(cid).await?;
                }
            }
        }

        if let Some(config) = &self.config {
            if let Some(path) = config.path.as_ref() {
                if let Err(_e) = tokio::fs::write(path.join(".index_id"), cid.to_string()).await {
                    error!("Error writing index: {_e}");
                }
            }
        }
        Ok(())
    }

    pub async fn import_index(&mut self) -> Result<()> {
        let ipfs = self.ipfs()?;
        if let Some(config) = &self.config {
            if let Some(path) = config.path.as_ref() {
                let cid_str = tokio::fs::read(path.join(".index_id"))
                    .await
                    .map(|bytes| String::from_utf8_lossy(&bytes).to_string())?;

                let cid: Cid = cid_str.parse().map_err(anyhow::Error::from)?;
                *self.index_cid.write() = Some(cid);

                let mut index_stream = ipfs
                    .unixfs()
                    .cat(IpfsPath::from(cid), None, &[], true)
                    .await
                    .map_err(anyhow::Error::from)?
                    .boxed();

                let mut data = vec![];

                while let Some(result) = index_stream.next().await {
                    let mut bytes = result.map_err(anyhow::Error::from)?;
                    data.append(&mut bytes);
                }

                let key = self.keypair()?;

                let index_bytes = ecdh_decrypt(&key, None, data)?;

                self.import(
                    ConstellationDataType::Json,
                    String::from_utf8_lossy(&index_bytes).to_string(),
                )?;
            }
        }
        Ok(())
    }

    pub fn keypair(&self) -> Result<Arc<DID>> {
        let kp = self.keypair.read().clone().ok_or(Error::Other)?;
        Ok(kp)
    }

    pub fn ipfs(&self) -> Result<Ipfs> {
        self.ipfs
            .read()
            .as_ref()
            .cloned()
            .ok_or(Error::ConstellationExtensionUnavailable)
    }
}

impl Extension for IpfsFileSystem {
    fn id(&self) -> String {
        String::from("warp-fs-ipfs")
    }
    fn name(&self) -> String {
        String::from("IPFS FileSystem")
    }

    fn module(&self) -> Module {
        Module::FileSystem
    }

    fn is_ready(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }

    fn extension_subscribe(
        &self,
    ) -> Result<futures::stream::BoxStream<'static, ExtensionEventKind>> {
        let mut rx = self.ready_tx.subscribe();
        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        };

        Ok(stream.boxed())
    }
}

impl SingleHandle for IpfsFileSystem {
    fn handle(&self) -> Result<Box<dyn Any>> {
        self.ipfs().map(|ipfs| Box::new(ipfs) as Box<dyn Any>)
    }
}

#[async_trait::async_trait]
impl Constellation for IpfsFileSystem {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> Directory {
        self.index.clone()
    }

    fn max_size(&self) -> usize {
        self.config
            .as_ref()
            .and_then(|config| config.max_storage_size)
            .unwrap_or(1024 * 1024 * 1024)
    }

    async fn put(&mut self, name: &str, path: &str) -> Result<ConstellationProgressStream> {
        let name = name.trim();
        if name.len() < 2 || name.len() > 256 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name.len(),
                minimum: Some(2),
                maximum: Some(256),
            });
        }

        let ipfs = self.ipfs()?;

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

        let ((width, height), exact) = self
            .config
            .as_ref()
            .map(|c| (c.thumbnail_size, c.thumbnail_exact_format))
            .unwrap_or(((128, 128), true));

        let ticket = self
            .thumbnail_store
            .insert(&path, width, height, exact)
            .await?;

        let background = self
            .config
            .as_ref()
            .map(|f| f.thumbnail_task)
            .unwrap_or_default();

        let name = name.to_string();
        let fs: IpfsFileSystem = self.clone();
        let progress_stream = async_stream::stream! {

            let mut last_written = 0;

            let mut total_written = 0;
            let mut returned_path = None;

            let mut stream = match ipfs.unixfs().add(path.clone(), Some(AddOption { pin: true, ..Default::default() })).await {
                Ok(ste) => ste,
                Err(e) => {
                    yield Progression::ProgressFailed {
                            name,
                            last_size: Some(last_written),
                            error: Some(e.to_string()),
                    };
                    return;
                }
            };

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
                        Ok((extension_type, thumbnail)) => {
                            file.set_thumbnail(&thumbnail);
                            file.set_thumbnail_format(extension_type.into());
                            //We export again so the thumbnail can be apart of the index
                            if background {
                                if let Err(_e) = fs.export_index().await {}
                            }
                        }
                        Err(_e) => {}
                    }
                }
            };

            if !background {
                task.await;
                if let Err(_e) = fs.export_index().await {}
            } else {
                tokio::spawn(task);
            }

            yield Progression::ProgressComplete {
                name: name.to_string(),
                total: Some(total_written),
            };

            let _ = fs.broadcast.send(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written)
            }).ok();
        };

        Ok(ConstellationProgressStream(progress_stream.boxed()))
    }

    async fn get(&self, name: &str, path: &str) -> Result<()> {
        let ipfs = self.ipfs()?;

        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let reference = file.reference().ok_or(Error::Other)?; //Reference not found

        let mut stream = ipfs
            .get_unixfs(reference.parse::<IpfsPath>()?, path)
            .await?;

        while let Some(status) = stream.next().await {
            if let UnixfsStatus::FailedStatus { error, .. } = status {
                return Err(error.map(Error::Any).unwrap_or(Error::Other));
            }
        }

        //TODO: Validate file against the hashed reference
        if let Err(_e) = self.broadcast.send(ConstellationEventKind::Downloaded {
            filename: file.name(),
            size: Some(file.size()),
            location: Some(PathBuf::from(path)),
        }) {}
        Ok(())
    }

    async fn put_buffer(&mut self, name: &str, buffer: &[u8]) -> Result<()> {
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

        let ipfs = self.ipfs()?;

        if self.current_directory()?.get_item_by_path(name).is_ok() {
            return Err(Error::FileExist);
        }

        let ((width, height), exact) = self
            .config
            .as_ref()
            .map(|c| (c.thumbnail_size, c.thumbnail_exact_format))
            .unwrap_or(((128, 128), true));

        let ticket = self
            .thumbnail_store
            .insert_buffer(&name, buffer, width, height, exact)
            .await;

        let reader = ReaderStream::new(Cursor::new(buffer))
            .map(|result| result.map(|x| x.into()))
            .boxed();

        let mut total_written = 0;
        let mut returned_path = None;

        let mut stream = ipfs
            .unixfs()
            .add(
                reader,
                Some(AddOption {
                    pin: true,
                    ..Default::default()
                }),
            )
            .await?;

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
            Ok((extension_type, thumbnail)) => {
                file.set_thumbnail(&thumbnail);
                file.set_thumbnail_format(extension_type.into())
            }
            Err(_e) => {}
        }

        self.current_directory()?.add_item(file)?;

        if let Err(_e) = self.export_index().await {}
        let _ = self
            .broadcast
            .send(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written),
            })
            .ok();
        Ok(())
    }

    async fn get_buffer(&self, name: &str) -> Result<Vec<u8>> {
        let ipfs = self.ipfs()?;

        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let reference = file.reference().ok_or(Error::Other)?; //Reference not found
        let stream = ipfs
            .cat_unixfs(reference.parse::<IpfsPath>()?, None)
            .await
            .map_err(anyhow::Error::from)?;
        pin_mut!(stream);

        let mut buffer = vec![];
        while let Some(data) = stream.next().await {
            let mut bytes = data.map_err(anyhow::Error::from)?;
            buffer.append(&mut bytes);
        }

        //TODO: Validate file against the hashed reference
        let _ = self
            .broadcast
            .send(ConstellationEventKind::Downloaded {
                filename: file.name(),
                size: Some(file.size()),
                location: None,
            })
            .ok();
        Ok(buffer)
    }

    /// Used to upload file to the filesystem with data from a stream
    async fn put_stream(
        &mut self,
        name: &str,
        total_size: Option<usize>,
        stream: BoxStream<'static, Vec<u8>>,
    ) -> Result<ConstellationProgressStream> {
        let name = name.trim();
        if name.len() < 2 || name.len() > 256 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name.len(),
                minimum: Some(2),
                maximum: Some(256),
            });
        }

        let ipfs = self.ipfs()?;
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

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

            let mut stream = match ipfs.unixfs().add(stream, Some(AddOption { pin: true, ..Default::default() })).await {
                Ok(ste) => ste,
                Err(e) => {
                    yield Progression::ProgressFailed {
                        name,
                        last_size: Some(last_written),
                        error: Some(e.to_string()),
                    };
                    return;
                }
            };

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

            if let Err(_e) = fs.export_index().await {}

            yield Progression::ProgressComplete {
                name: name.to_string(),
                total: Some(total_written),
            };

            let _ = fs.broadcast.send(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written)
            }).ok();
        };

        Ok(ConstellationProgressStream(progress_stream.boxed()))
    }

    /// Used to download data from the filesystem using a stream
    async fn get_stream(&self, name: &str) -> Result<BoxStream<'static, Result<Vec<u8>>>> {
        let ipfs = self.ipfs()?;
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let size = file.size();
        let reference = file.reference().ok_or(Error::Other)?; //Reference not found

        let tx = self.broadcast.clone();

        let stream = async_stream::stream! {
            let cat_stream = ipfs
                .cat_unixfs(reference.parse::<IpfsPath>()?, None)
                .await
                .map_err(anyhow::Error::from)?;

            for await data in cat_stream {
                match data {
                    Ok(data) => yield Ok(data),
                    Err(e) => yield Err(Error::from(anyhow::anyhow!("{e}"))),
                }
            }

            let _ = tx.send(ConstellationEventKind::Downloaded { filename: file.name(), size: Some(size), location: None }).ok();
        };

        //TODO: Validate file against the hashed reference
        Ok(stream.boxed())
    }

    /// Used to remove data from the filesystem
    async fn remove(&mut self, name: &str, _: bool) -> Result<()> {
        let ipfs = self.ipfs()?;
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

        let mut pinned_blocks: HashSet<_> = HashSet::from_iter(
            ipfs.list_pins(None)
                .await
                .filter_map(|r| async move {
                    match r {
                        Ok(v) => Some(v.0),
                        Err(_) => None,
                    }
                })
                .collect::<Vec<_>>()
                .await,
        );

        if ipfs.is_pinned(&cid).await? {
            ipfs.remove_pin(&cid, true).await?;
        }

        let new_pinned_blocks: HashSet<_> = HashSet::from_iter(
            ipfs.list_pins(None)
                .await
                .filter_map(|r| async move {
                    match r {
                        Ok(v) => Some(v.0),
                        Err(_) => None,
                    }
                })
                .collect::<Vec<_>>()
                .await,
        );

        for s_cid in new_pinned_blocks.iter() {
            pinned_blocks.remove(s_cid);
        }

        for cid in pinned_blocks {
            ipfs.remove_block(cid).await?;
        }

        directory.remove_item(&item.name())?;
        if let Err(_e) = self.export_index().await {}

        let _ = self
            .broadcast
            .send(ConstellationEventKind::Deleted {
                item_name: name.to_string(),
            })
            .ok();

        Ok(())
    }

    async fn rename(&mut self, current: &str, new: &str) -> Result<()> {
        //Used as guard in the event its not available but will be used in the future
        let _ipfs = self.ipfs()?;

        //Note: This will only support renaming the file or directory in the index
        let directory = self.current_directory()?;

        if directory.get_item_by_path(new).is_ok() {
            return Err(Error::DuplicateName);
        }

        directory.rename_item(current, new)?;
        if let Err(_e) = self.export_index().await {}
        let _ = self
            .broadcast
            .send(ConstellationEventKind::Renamed {
                old_item_name: current.to_string(),
                new_item_name: new.to_string(),
            })
            .ok();
        Ok(())
    }

    async fn create_directory(&mut self, name: &str, recursive: bool) -> Result<()> {
        //Used as guard in the event its not available but will be used in the future
        let _ipfs = self.ipfs()?;

        let directory = self.current_directory()?;

        //Prevent creating recursive/nested directorieis if `recursive` isnt true
        if name.contains('/') && !recursive {
            return Err(Error::InvalidDirectory);
        }

        if directory.has_item(name) || directory.get_item_by_path(name).is_ok() {
            return Err(Error::DirectoryExist);
        }

        self.current_directory()?
            .add_directory(Directory::new(name))?;
        if let Err(_e) = self.export_index().await {}
        Ok(())
    }

    async fn sync_ref(&mut self, path: &str) -> Result<()> {
        let ipfs = self.ipfs()?;

        let directory = self.current_directory()?;
        let file = directory
            .get_item_by_path(path)
            .and_then(|item| item.get_file())?;

        let reference = file.reference().ok_or(Error::FileNotFound)?;

        let stream = ipfs
            .cat_unixfs(reference.parse::<IpfsPath>()?, None)
            .await
            .map_err(anyhow::Error::from)?;

        pin_mut!(stream);

        let mut buffer = vec![];
        while let Some(data) = stream.next().await {
            let bytes = data.map_err(anyhow::Error::from)?;
            buffer.extend(bytes);
        }

        let Some(((width, height), exact)) = self
            .config
            .as_ref()
            .map(|c| (c.thumbnail_size, c.thumbnail_exact_format)) else {
                // No need to error since logic after only deals with thumbnail generation
                return Ok(());
            };

        // Generate the thumbnail for the file
        let id = self
            .thumbnail_store
            .insert_buffer(file.name(), &buffer, width, height, exact)
            .await;

        if let Ok((extension_type, thumbnail)) = self.thumbnail_store.get(id).await {
            file.set_thumbnail(&thumbnail);
            file.set_thumbnail_format(extension_type.into())
        }

        let _ = self.export_index().await;

        Ok(())
    }

    fn set_path(&mut self, path: PathBuf) {
        *self.path.write() = path;
    }

    fn get_path(&self) -> PathBuf {
        PathBuf::from(self.path.read().to_string_lossy().replace('\\', "/"))
    }
}

#[async_trait::async_trait]
impl ConstellationEvent for IpfsFileSystem {
    async fn subscribe(&mut self) -> Result<ConstellationEventStream> {
        let mut rx = self.broadcast.subscribe();

        let stream = async_stream::stream! {
            loop {
                match rx.recv().await {
                    Ok(event) => yield event,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(_) => {}
                };
            }
        };

        Ok(ConstellationEventStream(stream.boxed()))
    }
}

pub mod ffi {
    use crate::config::FsIpfsConfig;
    use crate::IpfsFileSystem;
    use warp::constellation::ConstellationAdapter;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_fs_ipfs_temporary_new(
        multipass: *const MultiPassAdapter,
        config: *const FsIpfsConfig,
    ) -> FFIResult<ConstellationAdapter> {
        let account = match multipass.is_null() {
            true => {
                return FFIResult::err(Error::NullPointerContext {
                    pointer: "multipass".into(),
                })
            }
            false => (*multipass).object(),
        };
        let config = match config.is_null() {
            true => None,
            false => Some((*config).clone()),
        };
        warp::async_on_block(IpfsFileSystem::new(account, config))
            .map(|fs| ConstellationAdapter::new(Box::new(fs)))
            .map_err(Error::from)
            .into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_fs_ipfs_persistent_new(
        multipass: *const MultiPassAdapter,
        config: *const FsIpfsConfig,
    ) -> FFIResult<ConstellationAdapter> {
        let account = match multipass.is_null() {
            true => {
                return FFIResult::err(Error::NullPointerContext {
                    pointer: "multipass".into(),
                })
            }
            false => (*multipass).object(),
        };
        let config = match config.is_null() {
            true => None,
            false => Some((*config).clone()),
        };

        warp::async_on_block(IpfsFileSystem::new(account, config))
            .map(|fs| ConstellationAdapter::new(Box::new(fs)))
            .map_err(Error::from)
            .into()
    }
}

fn ecdh_encrypt<K: AsRef<[u8]>>(did: &DID, recipient: Option<DID>, data: K) -> Result<Vec<u8>> {
    let prikey = Ed25519KeyPair::from_secret_key(&did.private_key_bytes()).get_x25519();
    let did_pubkey = match recipient {
        Some(did) => did.public_key_bytes(),
        None => did.public_key_bytes(),
    };

    let pubkey = Ed25519KeyPair::from_public_key(&did_pubkey).get_x25519();
    let prik = Zeroizing::new(prikey.key_exchange(&pubkey));
    let data = Cipher::direct_encrypt(data.as_ref(), &prik)?;

    Ok(data)
}

fn ecdh_decrypt<K: AsRef<[u8]>>(did: &DID, recipient: Option<DID>, data: K) -> Result<Vec<u8>> {
    let prikey = Ed25519KeyPair::from_secret_key(&did.private_key_bytes()).get_x25519();
    let did_pubkey = match recipient {
        Some(did) => did.public_key_bytes(),
        None => did.public_key_bytes(),
    };

    let pubkey = Ed25519KeyPair::from_public_key(&did_pubkey).get_x25519();
    let prik = Zeroizing::new(prikey.key_exchange(&pubkey));
    let data = Cipher::direct_decrypt(data.as_ref(), &prik)?;

    Ok(data)
}

pub(crate) fn to_file_type(name: &str) -> FileType {
    let name = PathBuf::from(name.trim());
    let extension = name
        .extension()
        .and_then(OsStr::to_str)
        .map(ExtensionType::from)
        .unwrap_or(ExtensionType::Other);

    extension.into()
}

pub fn get_keypair_did(keypair: &Keypair) -> anyhow::Result<DID> {
    let kp = Zeroizing::new(keypair.clone().try_into_ed25519()?.to_bytes());
    let kp = warp::crypto::ed25519_dalek::Keypair::from_bytes(&*kp)?;
    let did = DIDKey::Ed25519(Ed25519KeyPair::from_secret_key(kp.secret.as_bytes()));
    Ok(did.into())
}

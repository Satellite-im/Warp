pub mod config;
use config::FsIpfsConfig;
use futures::stream::BoxStream;
use futures::{pin_mut, StreamExt};
use ipfs::unixfs::UnixfsStatus;
use libipld::serde::{from_ipld, to_ipld};
use libipld::Cid;
use rust_ipfs as ipfs;
use std::any::Any;
use std::collections::HashSet;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::io::ReaderStream;
use warp::constellation::{
    ConstellationDataType, ConstellationEvent, ConstellationEventKind, ConstellationEventStream,
    ConstellationProgressStream, Progression,
};
use warp::logging::tracing::{debug, error};
use warp::multipass::MultiPass;
use warp::sata::{Kind, Sata};
use warp::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use warp::module::Module;

use chrono::{DateTime, Utc};
use ipfs::{Ipfs, IpfsPath};

use warp::constellation::{path::Path, directory::Directory, Constellation};
use warp::error::Error;
use warp::pocket_dimension::PocketDimension;
use warp::{Extension, SingleHandle};

type Result<T> = std::result::Result<T, Error>;

#[allow(clippy::type_complexity)]
pub struct IpfsFileSystem {
    index: Directory,
    path: Arc<RwLock<Path>>,
    modified: DateTime<Utc>,
    config: Option<FsIpfsConfig>,
    ipfs: Arc<RwLock<Option<Ipfs>>>,
    index_cid: Arc<RwLock<Option<Cid>>>,
    account: Arc<tokio::sync::RwLock<Option<Box<dyn MultiPass>>>>,
    broadcast: tokio::sync::broadcast::Sender<ConstellationEventKind>,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
}

impl Clone for IpfsFileSystem {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            path: self.path.clone(),
            modified: self.modified,
            config: self.config.clone(),
            ipfs: self.ipfs.clone(),
            index_cid: self.index_cid.clone(),
            account: self.account.clone(),
            broadcast: self.broadcast.clone(),
            cache: self.cache.clone(),
        }
    }
}

impl IpfsFileSystem {
    pub async fn new(
        account: Box<dyn MultiPass>,
        config: Option<FsIpfsConfig>,
    ) -> anyhow::Result<Self> {
        let (tx, _) = tokio::sync::broadcast::channel(1024);

        let filesystem = IpfsFileSystem {
            index: Directory::new("root"),
            path: Arc::new(Default::default()),
            modified: Utc::now(),
            config,
            index_cid: Default::default(),
            account: Default::default(),
            ipfs: Default::default(),
            broadcast: tx,
            cache: None,
        };

        *filesystem.account.write().await = Some(account);

        if let Some(account) = filesystem.account.read().await.clone() {
            if account.get_own_identity().await.is_err() {
                debug!("Identity doesnt exist. Waiting for it to load or to be created");
                let mut filesystem = filesystem.clone();

                tokio::spawn({
                    let account = account.clone();
                    async move {
                        loop {
                            match account.get_own_identity().await {
                                Ok(_) => break,
                                _ => {
                                    //TODO: have a flag that would tell is it been an error other than it not being available
                                    //      so we dont try to extract ipfs
                                    tokio::time::sleep(Duration::from_millis(100)).await
                                }
                            }
                        }
                        if let Err(e) = filesystem.initialize().await {
                            error!("Error initializing filesystem: {e}");
                        }
                    }
                });
            } else {
                let mut filesystem = filesystem.clone();
                filesystem.initialize().await?;
            }
        }

        Ok(filesystem)
    }

    async fn initialize(&mut self) -> Result<()> {
        debug!("Initializing or fetch ipfs");

        let account = self
            .account
            .read()
            .await
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)?;

        let ipfs_handle = match account.handle() {
            Ok(handle) if handle.is::<Ipfs>() => handle.downcast_ref::<Ipfs>().cloned(),
            _ => None,
        };

        let ipfs = ipfs_handle.ok_or(Error::ConstellationExtensionUnavailable)?;

        *self.ipfs.write() = Some(ipfs);

        if let Err(_e) = self.import_index().await {
            //TODO: Log error
        }

        tokio::spawn({
            let fs = self.clone();
            async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    if let Err(_e) = fs.export_index().await {
                        //Log error
                    }
                }
            }
        });

        Ok(())
    }

    #[allow(clippy::clone_on_copy)]
    pub async fn export_index(&self) -> Result<()> {
        let ipfs = self.ipfs()?;
        let mut object = Sata::default();
        let index = self.export(ConstellationDataType::Json)?;
        let data = match self.account().await {
            Ok(account) => {
                let key = account.decrypt_private_key(None)?;
                object.add_recipient(&key)?;
                object.encrypt(libipld::IpldCodec::DagJson, &key, Kind::Reference, index)?
            }
            _ => object.encode(libipld::IpldCodec::DagJson, Kind::Reference, index)?,
        };

        let ipld = to_ipld(data).map_err(anyhow::Error::from)?;
        let cid = ipfs.put_dag(ipld).await?;
        let last_cid = self.index_cid.read().clone();
        *self.index_cid.write() = Some(cid);
        if let Some(last_cid) = last_cid {
            if cid != last_cid {
                if ipfs.is_pinned(&last_cid).await? {
                    ipfs.remove_pin(&last_cid, false).await?;
                }
                ipfs.remove_block(last_cid).await?;
            }
        }

        if let Some(config) = &self.config {
            if let Some(path) = config.path.as_ref() {
                if let Err(_e) = tokio::fs::write(path.join(".index_id"), cid.to_string()).await {
                    //Log export?
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

                let ipld = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    ipfs.get_dag(IpfsPath::from(cid)),
                )
                .await
                .map_err(anyhow::Error::from)??;

                let data: Sata = from_ipld(ipld).map_err(anyhow::Error::from)?;

                let index: String = match self.account().await {
                    Ok(account) => {
                        let key = account.decrypt_private_key(None)?;
                        data.decrypt(&key)?
                    }
                    _ => data.decode()?,
                };

                self.import(ConstellationDataType::Json, index)?;
            }
        }
        Ok(())
    }

    pub async fn account(&self) -> Result<Box<dyn MultiPass>> {
        self.account
            .read()
            .await
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
    }

    pub fn ipfs(&self) -> Result<Ipfs> {
        self.ipfs
            .read()
            .as_ref()
            .cloned()
            .ok_or(Error::ConstellationExtensionUnavailable)
    }

    pub fn get_cache(&self) -> anyhow::Result<RwLockReadGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or(Error::PocketDimensionExtensionUnavailable)?;

        let inner = cache.read();
        Ok(inner)
    }

    pub fn get_cache_mut(&self) -> anyhow::Result<RwLockWriteGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = cache.write();
        Ok(inner)
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

    async fn put(&mut self, name: &str, path: &str) -> Result<()> {
        let ipfs = self.ipfs()?;
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

        if self.current_directory()?.get_item_by_path(name).is_ok() {
            return Err(Error::DuplicateName);
        }

        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(Error::FileNotFound);
        }

        let mut total_written = 0;
        let mut returned_path = None;

        let mut stream = ipfs.add_file_unixfs(path.clone()).await?;

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
        let cid = ipfs_path.root().cid().ok_or(Error::Other)?;

        ipfs.insert_pin(cid, true).await?;
        let file = warp::constellation::file::File::new(name);
        file.set_size(total_written);
        file.set_reference(&format!("{ipfs_path}"));
        let f = file.clone();
        tokio::task::spawn_blocking(move || f.hash_mut().hash_from_file(&path))
            .await
            .map_err(anyhow::Error::from)??;
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

    async fn get(&self, name: &str, path: &str) -> Result<()> {
        let ipfs = self.ipfs()?;
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

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
            location: Some(Path::from(path)),
        }) {}
        Ok(())
    }

    async fn put_buffer(&mut self, name: &str, buffer: &Vec<u8>) -> Result<()> {
        let ipfs = self.ipfs()?;
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

        if self.current_directory()?.get_item_by_path(name).is_ok() {
            return Err(Error::FileExist);
        }

        let reader = ReaderStream::new(Cursor::new(buffer.clone()))
            .map(|result| result.map(|x| x.into()))
            .boxed();

        let mut total_written = 0;
        let mut returned_path = None;

        let mut stream = ipfs.add_unixfs(reader).await?;

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

        let cid = ipfs_path
            .root()
            .cid()
            .ok_or_else(|| anyhow::anyhow!("Cid was never set"))?;

        ipfs.insert_pin(cid, true).await?;

        let file = warp::constellation::file::File::new(name);
        file.set_size(total_written);
        file.set_reference(&format!("{ipfs_path}"));
        file.hash_mut().hash_from_slice(buffer)?;
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
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

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

            let mut stream = match ipfs.add_unixfs(stream).await {
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
                    UnixfsStatus::CompletedStatus { path, written, .. } => {
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
                        // return Err(error.map(Error::Any).unwrap_or(Error::Other));
                    }
                    UnixfsStatus::ProgressStatus { written, .. } => {
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
            let cid = match ipfs_path
                .root()
                .cid() {
                    Some(cid) => cid,
                    None => {
                        yield Progression::ProgressFailed {
                            name,
                            last_size: Some(last_written),
                            error: Some("Cid not set".into()),
                        };
                        return;
                    }
                };
            if let Err(e) = ipfs.insert_pin(cid, true).await {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: Some(e.to_string()),
                };
                return;
            }
            let file = warp::constellation::file::File::new(&name);
            file.set_size(total_written);
            file.set_reference(&format!("{ipfs_path}"));
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
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

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
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

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
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

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
        let handle = warp::async_handle();
        let _g = handle.enter();

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

        while let Some(data) = stream.next().await {
            //This is to make sure there isnt any errors while tranversing the links
            //however we do not need to deal with the data itself as the will store
            //it in the blockstore after being found or blocks being exchanged from peer
            //TODO: Should check first chunk and timeout if it exceeds a specific length
            let _ = data.map_err(anyhow::Error::from)?;
        }

        Ok(())
    }

    fn set_path(&mut self, path: Path) {
        *self.path.write() = path;
    }

    fn get_path(&self) -> Path {
        self.path.read().clone()
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

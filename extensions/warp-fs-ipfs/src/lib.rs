pub mod config;
use config::FsIpfsConfig;
use futures::{pin_mut, StreamExt};
use libipld::serde::{from_ipld, to_ipld};
use libipld::Cid;
use std::any::Any;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use warp::constellation::ConstellationDataType;
use warp::logging::tracing::{debug, error};
use warp::multipass::MultiPass;
use warp::sata::{Kind, Sata};
use warp::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
// use warp_common::futures::TryStreamExt;
use warp::module::Module;

use ipfs::{unixfs::ll::file::adder::FileAdder, Block};

use chrono::{DateTime, Utc};
use ipfs::{Ipfs, IpfsPath, IpfsTypes, TestTypes, Types};

use warp::constellation::{directory::Directory, Constellation};
use warp::error::Error;
use warp::pocket_dimension::PocketDimension;
use warp::{Extension, SingleHandle};

pub type Temporary = TestTypes;
pub type Persistent = Types;

type Result<T> = std::result::Result<T, Error>;

#[allow(clippy::type_complexity)]
pub struct IpfsFileSystem<T: IpfsTypes> {
    index: Directory,
    path: PathBuf,
    max_size: Arc<AtomicUsize>,
    modified: DateTime<Utc>,
    config: Option<FsIpfsConfig>,
    ipfs: Arc<RwLock<Option<Ipfs<T>>>>,
    index_cid: Arc<RwLock<Option<Cid>>>,
    account: Arc<tokio::sync::RwLock<Option<Arc<RwLock<Box<dyn MultiPass>>>>>>,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
}

impl<T: IpfsTypes> Clone for IpfsFileSystem<T> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            path: self.path.clone(),
            modified: self.modified,
            max_size: self.max_size.clone(),
            config: self.config.clone(),
            ipfs: self.ipfs.clone(),
            index_cid: self.index_cid.clone(),
            account: self.account.clone(),
            cache: self.cache.clone(),
        }
    }
}

impl<T: IpfsTypes> IpfsFileSystem<T> {
    pub async fn new(
        account: Arc<RwLock<Box<dyn MultiPass>>>,
        config: Option<FsIpfsConfig>,
    ) -> anyhow::Result<Self> {
        let filesystem = IpfsFileSystem {
            index: Directory::new("root"),
            path: PathBuf::new(),
            modified: Utc::now(),
            config,
            max_size: Arc::new(AtomicUsize::new(4 * 1024 * 1024)),
            index_cid: Default::default(),
            account: Default::default(),
            ipfs: Default::default(),
            cache: None,
        };

        *filesystem.account.write().await = Some(account);

        if let Some(account) = filesystem.account.read().await.clone() {
            if account.read().get_own_identity().is_err() {
                debug!("Identity doesnt exist. Waiting for it to load or to be created");
                let mut filesystem = filesystem.clone();

                tokio::spawn({
                    let account = account.clone();
                    async move {
                        loop {
                            match account.get_own_identity() {
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
            Ok(handle) if handle.is::<Ipfs<T>>() => handle.downcast_ref::<Ipfs<T>>().cloned(),
            _ => None,
        };

        let ipfs = match ipfs_handle {
            Some(ipfs) => ipfs,
            None => return Err(Error::ConstellationExtensionUnavailable),
        };

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
            if ipfs.is_pinned(&last_cid).await? {
                ipfs.remove_pin(&last_cid, false).await?;
            }
            ipfs.remove_block(last_cid).await?;
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

    pub async fn account(&self) -> Result<Arc<RwLock<Box<dyn MultiPass>>>> {
        self.account
            .read()
            .await
            .clone()
            .ok_or(Error::MultiPassExtensionUnavailable)
    }

    pub fn ipfs(&self) -> Result<Ipfs<T>> {
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

impl<T: IpfsTypes> Extension for IpfsFileSystem<T> {
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

impl<T: IpfsTypes> SingleHandle for IpfsFileSystem<T> {
    fn handle(&self) -> Result<Box<dyn Any>> {
        self.ipfs().map(|ipfs| Box::new(ipfs) as Box<dyn Any>)
    }
}

#[async_trait::async_trait]
impl<T: IpfsTypes> Constellation for IpfsFileSystem<T> {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> Directory {
        self.index.clone()
    }

    fn get_path_mut(&mut self) -> &mut PathBuf {
        &mut self.path
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

        let mut adder = FileAdder::default();
        let file = tokio::fs::File::open(&path).await?;

        //TODO: use a larger buffer(?)
        let mut reader = BufReader::with_capacity(adder.size_hint(), file);
        let mut written = 0;
        let mut last_cid = None;

        {
            let ipfs = ipfs.clone();
            loop {
                match reader.fill_buf().await? {
                    buffer if buffer.is_empty() => {
                        let blocks = adder.finish();
                        for (cid, block) in blocks {
                            let block = Block::new(cid, block)?;
                            let cid = ipfs.put_block(block).await?;
                            last_cid = Some(cid);
                        }
                        break;
                    }
                    buffer => {
                        let mut total = 0;

                        while total < buffer.len() {
                            let (blocks, consumed) = adder.push(&buffer[total..]);
                            for (cid, block) in blocks {
                                let block = Block::new(cid, block)?;
                                let _cid = ipfs.put_block(block).await?;
                            }
                            total += consumed;
                            written += consumed;
                        }
                        reader.consume(total);
                    }
                }
            }
        }
        let cid = last_cid.ok_or_else(|| anyhow::anyhow!("Cid was never set"))?;
        ipfs.insert_pin(&cid, true).await?;
        let file = warp::constellation::file::File::new(name);
        file.set_size(written);
        file.set_reference(&format!("/ipfs/{cid}"));
        file.hash_mut().hash_from_file(&path)?;
        self.current_directory()?.add_item(file)?;
        if let Err(_e) = self.export_index().await {}
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
        let stream = ipfs
            .cat_unixfs(reference.parse::<IpfsPath>()?, None)
            .await
            .map_err(anyhow::Error::from)?;
        pin_mut!(stream);
        let mut file = tokio::fs::File::create(path).await?;
        while let Some(data) = stream.next().await {
            let bytes = data.map_err(anyhow::Error::from)?;
            file.write_all(&bytes).await?;
            file.flush().await?;
        }
        //TODO: Validate file against the hashed reference
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

        let mut adder = FileAdder::default();
        //Maybe write directly to avoid reallocation?
        let mut reader =
            BufReader::with_capacity(adder.size_hint(), Cursor::new(buffer.as_slice()));
        let mut written = 0;
        let mut last_cid = None;

        {
            let ipfs = ipfs.clone();
            loop {
                match reader.fill_buf().await? {
                    buffer if buffer.is_empty() => {
                        let blocks = adder.finish();
                        for (cid, block) in blocks {
                            let block = Block::new(cid, block)?;
                            let cid = ipfs.put_block(block).await?;
                            last_cid = Some(cid);
                        }
                        break;
                    }
                    buffer => {
                        let mut total = 0;

                        while total < buffer.len() {
                            let (blocks, consumed) = adder.push(&buffer[total..]);
                            for (cid, block) in blocks {
                                let block = Block::new(cid, block)?;
                                let _cid = ipfs.put_block(block).await?;
                            }
                            total += consumed;
                            written += consumed;
                        }
                        reader.consume(total);
                    }
                }
            }
        }
        let cid = last_cid.ok_or_else(|| anyhow::anyhow!("Cid was never set"))?;

        ipfs.insert_pin(&cid, true).await?;

        let file = warp::constellation::file::File::new(name);
        file.set_size(written);
        file.set_reference(&format!("/ipfs/{cid}"));
        file.hash_mut().hash_from_slice(buffer)?;
        self.current_directory()?.add_item(file)?;
        if let Err(_e) = self.export_index().await {}
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
        Ok(buffer)
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

        if ipfs.is_pinned(&cid).await? {
            ipfs.remove_pin(&cid, true).await?;
        }

        ipfs.remove_block(cid).await?;
        directory.remove_item(&item.name())?;
        if let Err(_e) = self.export_index().await {}
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

        directory.rename_item(current, new)?;
        if let Err(_e) = self.export_index().await {}
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

        tokio::spawn(async move {

            let stream = ipfs
                .cat_unixfs(reference.parse::<IpfsPath>()?, None)
                .await
                .map_err(anyhow::Error::from)?;

            pin_mut!(stream);

            while let Some(data) = stream.next().await {
                let _ = data.map_err(anyhow::Error::from)?;
            }
            Ok::<_, Error>(())
        });

        Ok(())
    }

    fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    fn get_path(&self) -> PathBuf {
        self.path.clone()
    }
}

pub mod ffi {
    use crate::config::FsIpfsConfig;
    use crate::{IpfsFileSystem, Persistent, Temporary};
    use warp::constellation::ConstellationAdapter;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use warp::multipass::MultiPassAdapter;
    use warp::sync::{Arc, RwLock};

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
            false => (*multipass).inner(),
        };
        let config = match config.is_null() {
            true => None,
            false => Some((*config).clone()),
        };
        warp::async_on_block(IpfsFileSystem::<Temporary>::new(account, config))
            .map(|account| ConstellationAdapter::new(Arc::new(RwLock::new(Box::new(account)))))
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
            false => (*multipass).inner(),
        };
        let config = match config.is_null() {
            true => None,
            false => Some((*config).clone()),
        };

        warp::async_on_block(IpfsFileSystem::<Persistent>::new(account, config))
            .map(|account| ConstellationAdapter::new(Arc::new(RwLock::new(Box::new(account)))))
            .map_err(Error::from)
            .into()
    }
}

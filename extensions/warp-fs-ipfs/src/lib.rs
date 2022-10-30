use futures::{pin_mut, StreamExt};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use warp::logging::tracing::{debug, error};
use warp::multipass::MultiPass;
use warp::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
// use warp_common::futures::TryStreamExt;
use warp::module::Module;

use ipfs::{unixfs::ll::file::adder::FileAdder, Block};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use ipfs::{Ipfs, IpfsPath, IpfsTypes};

use warp::constellation::{directory::Directory, Constellation};
use warp::error::Error;
use warp::pocket_dimension::PocketDimension;
use warp::{Extension, SingleHandle};

type Result<T> = std::result::Result<T, Error>;

pub struct IpfsFileSystem<T: IpfsTypes> {
    index: Directory,
    path: PathBuf,
    modified: DateTime<Utc>,
    ipfs: Arc<RwLock<Option<Ipfs<T>>>>,
    account: Arc<RwLock<Box<dyn MultiPass>>>,
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
}

impl<T: IpfsTypes> Clone for IpfsFileSystem<T> {
    fn clone(&self) -> Self {
        Self {
            index: self.index.clone(),
            path: self.path.clone(),
            modified: self.modified,
            ipfs: self.ipfs.clone(),
            account: self.account.clone(),
            cache: self.cache.clone(),
        }
    }
}

impl<T: IpfsTypes> IpfsFileSystem<T> {
    pub async fn new(account: Arc<RwLock<Box<dyn MultiPass>>>) -> anyhow::Result<Self> {
        let mut filesystem = IpfsFileSystem {
            index: Directory::new("root"),
            path: PathBuf::new(),
            modified: Utc::now(),
            account,
            ipfs: Default::default(),
            cache: None,
        };

        if filesystem.account.read().get_own_identity().is_err() {
            debug!("Identity doesnt exist. Waiting for it to load or to be created");
            let mut filesystem = filesystem.clone();
            tokio::spawn(async move {
                loop {
                    match filesystem.account.get_own_identity() {
                        Ok(_) => break,
                        _ => {
                            //TODO: have a flag that would tell is it been an error other than it not being available
                            //      so we dont try to extract ipfs
                            tokio::time::sleep(Duration::from_millis(100)).await
                        }
                    }
                }
                if let Err(e) = filesystem.initialize().await {
                    error!("Error initializing store: {e}");
                }
            });
        } else {
            filesystem.initialize().await?;
        }

        Ok(filesystem)
    }

    async fn initialize(&mut self) -> Result<()> {
        debug!("Initializing internal store");

        let ipfs_handle = match self.account.handle() {
            Ok(handle) if handle.is::<Ipfs<T>>() => handle.downcast_ref::<Ipfs<T>>().cloned(),
            _ => None,
        };

        let ipfs = match ipfs_handle {
            Some(ipfs) => ipfs,
            None => return Err(Error::ConstellationExtensionUnavailable),
        };

        *self.ipfs.write() = Some(ipfs);

        Ok(())
    }

    pub fn ipfs(&self) -> Result<Ipfs<T>> {
        self.ipfs
            .read()
            .as_ref()
            .cloned()
            .ok_or(Error::ConstellationExtensionUnavailable)
    }

    pub fn set_cache(&mut self, cache: Arc<RwLock<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn get_cache(&self) -> anyhow::Result<RwLockReadGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("Pocket Dimension Extension is not set"))?;

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

impl<T: IpfsTypes> SingleHandle for IpfsFileSystem<T> {}

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

        if self.current_directory()?.get_item_by_path(name).is_ok() {
            return Err(Error::Other); //TODO: Exist
        }
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(Error::IoError(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )));
        }

        let mut adder = FileAdder::default();
        let file = tokio::fs::File::open(&path).await?;
        //TODO: use a larger buffer
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

        let file = warp::constellation::file::File::new(name);
        file.set_size(written);
        file.set_reference(&format!("/ipfs/{cid}"));
        file.hash_mut().hash_from_file(&path)?;
        self.current_directory()?.add_item(file)?;

        Ok(())
    }

    async fn get(&self, name: &str, path: &str) -> Result<()> {
        let ipfs = self.ipfs()?;
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

    async fn put_buffer(&mut self, _name: &str, _buffer: &Vec<u8>) -> Result<()> {
        let _ipfs = self.ipfs()?;
        Err(Error::Unimplemented)
    }

    async fn get_buffer(&self, _name: &str) -> Result<Vec<u8>> {
        let _ipfs = self.ipfs()?;
        Err(Error::Unimplemented)
    }

    fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    fn get_path(&self) -> PathBuf {
        self.path.clone()
    }
}

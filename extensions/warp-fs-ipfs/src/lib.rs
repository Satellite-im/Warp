use http::uri::Scheme;
use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, TryFromUri};
use std::io::ErrorKind;
use std::sync::{Arc, Mutex};
// use warp_common::futures::TryStreamExt;
use warp_module::Module;

use warp_common::{
    anyhow,
    chrono::{DateTime, Utc},
    error::Error,
    futures::TryStreamExt,
    serde::{Deserialize, Serialize},
    tokio,
    tokio_util::io::StreamReader,
    Extension,
};
use warp_constellation::item::Item;
use warp_constellation::{constellation::Constellation, directory::Directory};
use warp_pocket_dimension::PocketDimension;

#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct IpfsFileSystem {
    pub index: Directory,
    pub modified: DateTime<Utc>,
    #[serde(skip)]
    pub client: IpfsClient,
    #[serde(skip)]
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
}

impl Default for IpfsFileSystem {
    fn default() -> IpfsFileSystem {
        IpfsFileSystem {
            index: Directory::new("root"),
            modified: Utc::now(),
            client: IpfsClient::default(),
            cache: None,
        }
    }
}

impl IpfsFileSystem {
    pub fn new() -> Self {
        IpfsFileSystem::default()
    }
    pub fn new_with_host<S: AsRef<str>>(host: S, port: u16) -> anyhow::Result<Self> {
        let mut system = IpfsFileSystem::default();
        let client = IpfsClient::from_host_and_port(Scheme::HTTP, host.as_ref(), port)?;
        system.client = client;
        Ok(system)
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

#[warp_common::async_trait::async_trait]
impl Constellation for IpfsFileSystem {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.index
    }

    fn root_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }

    async fn put(&mut self, name: &str, path: &str) -> warp_common::Result<()> {
        //TODO: Implement a remote check along with a check within constellation to determine if the file exist
        if self.root_directory().get_child_by_path(name).is_ok() {
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::AlreadyExists,
            )));
        }

        if !name.starts_with('/') {
            return Err(Error::InvalidPath);
        };

        let fs = std::fs::File::open(path)?;
        let size = fs.metadata()?.len();

        self.client
            .files_write(name, true, true, fs)
            .await
            .map_err(|_| Error::ToBeDetermined)?;

        // Get the file stat from ipfs
        let stat = self
            .client
            .files_stat(name)
            .await
            .map_err(|_| Error::ToBeDetermined)?;

        //check and compare size and if its different from local size to error

        if stat.size != size {
            //Delete from ipfs
            self.client
                .files_rm(name, false)
                .await
                .map_err(|_| Error::ToBeDetermined)?;

            return Err(Error::ToBeDetermined);
        }

        let hash = stat.hash;

        //pin file since ipfs mfs doesnt do it automatically

        let res = self
            .client
            .pin_add(&hash, true)
            .await
            .map_err(|_| Error::ToBeDetermined)?;

        if !res.pins.contains(&hash) {
            //TODO: Error?
        }

        let mut file = warp_constellation::file::File::new(&name[1..]);
        file.set_size(size as i64);
        file.set_hash(hash);

        self.open_directory("")?.add_child(file)?;

        self.modified = Utc::now();
        Ok(())
    }

    async fn get(&self, name: &str, path: &str) -> warp_common::Result<()> {
        // TODO: Implement a function that would check against both remote and constellation
        //       otherwise this would give an error if it doesnt exist within constellation
        //       even if it exist remotely
        // if self.root_directory().get_child_by_path(name).is_err() {
        //     return Err(warp_common::error::Error::IoError(std::io::Error::from(
        //         ErrorKind::NotFound,
        //     )));
        // }

        if !name.starts_with('/') {
            return Err(Error::InvalidPath);
        };

        // let file = self
        //     .root_directory()
        //     .get_child_by_path(&name)
        //     .and_then(Item::get_file)?;

        let mut fs = tokio::fs::File::create(path).await?;

        let stream = self
            .client
            .files_read(name)
            .map_err(|_| std::io::Error::from(ErrorKind::Other));

        let mut reader_stream = StreamReader::new(stream);

        tokio::io::copy(&mut reader_stream, &mut fs).await?;

        Ok(())
    }

    async fn from_buffer(&mut self, name: &str, buffer: &Vec<u8>) -> warp_common::Result<()> {
        if !name.starts_with('/') {
            return Err(Error::InvalidPath);
        };

        let fs = std::io::Cursor::new(buffer.clone());

        self.client
            .files_write(name, true, true, fs)
            .await
            .map_err(|_| Error::ToBeDetermined)?;

        // Get the file stat from ipfs
        let stat = self
            .client
            .files_stat(name)
            .await
            .map_err(|_| Error::ToBeDetermined)?;

        //check and compare size and if its different from local size to error
        let size = buffer.len() as u64;

        if stat.size != size {
            //Delete from ipfs
            self.client
                .files_rm(name, false)
                .await
                .map_err(|_| Error::ToBeDetermined)?;

            return Err(Error::ToBeDetermined);
        }

        let hash = stat.hash;

        let res = self
            .client
            .pin_add(&hash, true)
            .await
            .map_err(|_| Error::ToBeDetermined)?;

        if !res.pins.contains(&hash) {
            //TODO: Error?
        }

        let mut file = warp_constellation::file::File::new(&name[1..]);
        file.set_size(size as i64);
        file.set_hash(hash);

        self.open_directory("")?.add_child(file)?;

        self.modified = Utc::now();
        Ok(())
    }

    async fn to_buffer(&self, name: &str, buffer: &mut Vec<u8>) -> warp_common::Result<()> {
        if !name.starts_with('/') {
            return Err(Error::InvalidPath);
        };

        let _file = self
            .root_directory()
            .get_child(&name[1..])
            .and_then(Item::get_file)?;

        let stream = self
            .client
            .files_read(name)
            .map_err(|_| std::io::Error::from(ErrorKind::Other));

        let mut reader_stream = StreamReader::new(stream);

        tokio::io::copy(&mut reader_stream, buffer).await?;
        Ok(())
    }

    async fn remove(&mut self, name: &str) -> warp_common::Result<()> {
        if !name.starts_with('/') {
            return Err(Error::InvalidPath);
        };

        //TODO: Resolve to full directory
        if !self.root_directory().has_child(&name[1..]) {
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::NotFound,
            )));
        }

        self.client
            .files_rm(name, false)
            .await
            .map_err(|_| Error::ToBeDetermined)?;

        self.root_directory_mut().remove_child(&name[1..])?;
        Ok(())
    }
}

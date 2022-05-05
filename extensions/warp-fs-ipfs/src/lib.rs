use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, TryFromUri};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
// use warp_common::futures::TryStreamExt;
use warp::module::Module;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use tokio_util::io::StreamReader;
use warp::constellation::item::Item;
use warp::constellation::{directory::Directory, Constellation};
use warp::data::{DataObject, DataType};
use warp::error::Error;
use warp::hooks::Hooks;
use warp::pocket_dimension::query::QueryBuilder;
use warp::pocket_dimension::{DimensionData, PocketDimension};
use warp::Extension;

type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize, Deserialize, Clone)]
pub struct IpfsFileSystem {
    pub index: Directory,
    path: PathBuf,
    pub modified: DateTime<Utc>,
    #[serde(skip)]
    pub client: IpfsInternalClient,
    #[serde(skip)]
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    #[serde(skip)]
    pub hooks: Option<Arc<Mutex<Hooks>>>,
}

#[derive(Default, Clone)]
pub struct IpfsInternalClient {
    pub client: IpfsClient<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>,
    pub option: IpfsOption,
}

#[derive(Clone)]
pub enum IpfsOption {
    Mfs,
    Object,
}

impl Default for IpfsOption {
    fn default() -> Self {
        Self::Mfs
    }
}

impl IpfsInternalClient {
    pub fn new(
        client: IpfsClient<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>,
        option: IpfsOption,
    ) -> Self {
        Self { client, option }
    }
}

impl AsRef<IpfsClient<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>>
    for IpfsInternalClient
{
    fn as_ref(&self) -> &IpfsClient<hyper_tls::HttpsConnector<hyper::client::HttpConnector>> {
        &self.client
    }
}

impl From<IpfsClient<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>>
    for IpfsInternalClient
{
    fn from(client: IpfsClient<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>) -> Self {
        Self {
            client,
            ..Default::default()
        }
    }
}

impl Default for IpfsFileSystem {
    fn default() -> IpfsFileSystem {
        IpfsFileSystem {
            index: Directory::new("root"),
            path: PathBuf::new(),
            modified: Utc::now(),
            client: IpfsInternalClient::default(),
            cache: None,
            hooks: None,
        }
    }
}

impl IpfsFileSystem {
    pub fn new() -> Self {
        IpfsFileSystem::default()
    }

    pub fn new_with_uri<S: AsRef<str>>(uri: S) -> anyhow::Result<Self> {
        let mut system = IpfsFileSystem::default();
        let client =
            IpfsClient::<hyper_tls::HttpsConnector<hyper::client::HttpConnector>>::from_str(
                uri.as_ref(),
            )?;
        system.client = IpfsInternalClient::from(client);
        Ok(system)
    }

    pub fn set_cache(&mut self, cache: Arc<Mutex<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hook: Arc<Mutex<Hooks>>) {
        self.hooks = Some(hook)
    }

    pub fn get_cache(&self) -> anyhow::Result<MutexGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = match cache.lock() {
            Ok(inner) => inner,
            Err(e) => e.into_inner(),
        };

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

#[async_trait::async_trait]
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

    fn get_path_mut(&mut self) -> &mut PathBuf {
        &mut self.path
    }

    async fn put(&mut self, name: &str, path: &str) -> Result<()> {
        //TODO: Implement a remote check along with a check within constellation to determine if the file exist
        if self.root_directory().get_item_by_path(name).is_ok() {
            return Err(warp::error::Error::IoError(std::io::Error::from(
                ErrorKind::AlreadyExists,
            )));
        }

        let name = affix_root(name);

        let fs = std::fs::File::open(path)?;
        let size = fs.metadata()?.len();
        let client = self.client.as_ref();

        let hash = match self.client.option {
            IpfsOption::Mfs => {
                client
                    .files_write(&name, true, true, fs)
                    .await
                    .map_err(|e| anyhow!(e))?;

                // Get the file stat from ipfs
                let stat = client.files_stat(&name).await.map_err(|e| anyhow!(e))?;

                //check and compare size and if its different from local size to error

                if stat.size != size {
                    //Delete from ipfs
                    client
                        .files_rm(&name, false)
                        .await
                        .map_err(|e| anyhow!(e))?;

                    return Err(Error::Any(anyhow!(
                        "Size of the file does not match what was uploaded"
                    )));
                }

                let hash = stat.hash;

                //Note: MFS will pin files at the root of the system, but any stored
                //      in directories will not be automatically pinned.
                let res = client.pin_add(&hash, true).await.map_err(|e| anyhow!(e))?;

                if !res.pins.contains(&hash) {
                    //TODO: Error?
                }

                hash
            }
            IpfsOption::Object => {
                let res = client.add(fs).await.map_err(|e| anyhow!(e))?;

                //pin file since ipfs mfs doesnt do it automatically

                //TODO: Give a choice to pin file or not

                // let res = client
                //     .pin_add(&hash, true)
                //     .await
                //     .map_err(|_| Error::ToBeDetermined)?;
                //
                // if !res.pins.contains(&hash) {
                //     //TODO: Error?
                // }

                res.hash
            }
        };

        let mut file = warp::constellation::file::File::new(&name[1..]);
        file.set_size(size as i64);

        file.hash_mut().hash_from_file(path)?;

        file.set_ref(&hash);

        self.current_directory_mut()?.add_item(file.clone())?;

        self.modified = Utc::now();

        if let Ok(mut cache) = self.get_cache() {
            let object = DataObject::new(
                DataType::from(Module::FileSystem),
                DimensionData::from_path(path),
            )?;
            cache.add_data(DataType::from(Module::FileSystem), &object)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), file)?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::new_file", &object)
        }

        Ok(())
    }

    async fn get(&self, name: &str, path: &str) -> Result<()> {
        // TODO: Implement a function that would check against both remote and constellation
        //       otherwise this would give an error if it doesnt exist within constellation
        //       even if it exist remotely
        // if self.root_directory().get_item_by_path(name).is_err() {
        //     return Err(warp_common::error::Error::IoError(std::io::Error::from(
        //         ErrorKind::NotFound,
        //     )));
        // }

        let name = affix_root(name);

        if let Ok(cache) = self.get_cache() {
            let name = Path::new(&name)
                .file_name()
                .ok_or(Error::Other)?
                .to_string_lossy()
                .to_string();

            let mut query = QueryBuilder::default();
            query.r#where("name", &name)?;
            if let Ok(list) = cache.get_data(DataType::from(Module::FileSystem), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let obj = list.last().unwrap();
                    if let Ok(data) = obj.payload::<DimensionData>() {
                        if let Ok(mut file) = std::fs::File::create(path) {
                            data.write_from_path(&mut file)?;
                            return Ok(());
                        }
                    }
                }
            }
        }

        let _file = self
            .current_directory()
            .get_item_by_path(&name)
            .and_then(Item::get_file)?;

        let mut fs = tokio::fs::File::create(path).await?;

        let client = self.client.as_ref();

        match self.client.option {
            IpfsOption::Mfs => {
                let stream = client
                    .files_read(&name)
                    .map_err(|_| std::io::Error::from(ErrorKind::Other));

                let mut reader_stream = StreamReader::new(stream);

                tokio::io::copy(&mut reader_stream, &mut fs).await?;

                //Compare size though here we should compare hash instead.

                let size = tokio::fs::metadata(path).await?.len();

                let stat = client.files_stat(&name).await.map_err(|e| anyhow!(e))?;

                //check and compare size and if its different from local size to error
                //TODO: Compare hashes instead
                if stat.size != size {
                    tokio::fs::remove_file(path).await?;
                    return Err(Error::Any(anyhow!("File downloaded was invalid")));
                }
            }
            _ => return Err(Error::Unimplemented),
        };

        Ok(())
    }

    async fn from_buffer(&mut self, name: &str, buffer: &Vec<u8>) -> Result<()> {
        let name = affix_root(name);

        let fs = std::io::Cursor::new(buffer.clone());
        let client = self.client.as_ref();

        let mut file = warp::constellation::file::File::new(&name[1..]);

        let hash = match self.client.option {
            IpfsOption::Mfs => {
                client
                    .files_write(&name, true, true, fs)
                    .await
                    .map_err(|e| anyhow!(e))?;
                // Get the file stat from ipfs
                let stat = client.files_stat(&name).await.map_err(|e| anyhow!(e))?;

                //check and compare size and if its different from local size to error
                let size = buffer.len() as u64;

                if stat.size != size {
                    //Delete from ipfs
                    client
                        .files_rm(&name, false)
                        .await
                        .map_err(|e| anyhow!(e))?;

                    return Err(Error::Any(anyhow!("File downloaded was invalid")));
                }
                file.set_size(size as i64);

                let hash = stat.hash;

                let res = client.pin_add(&hash, true).await.map_err(|e| anyhow!(e))?;

                if !res.pins.contains(&hash) {
                    //TODO: Error?
                }

                hash
            }
            _ => return Err(Error::Unimplemented),
        };
        file.hash_mut().hash_from_slice(buffer);
        file.set_ref(&hash);

        self.current_directory_mut()?.add_item(file.clone())?;

        self.modified = Utc::now();

        if let Ok(mut cache) = self.get_cache() {
            let name = Path::new(&name)
                .file_name()
                .ok_or(Error::Other)?
                .to_string_lossy()
                .to_string();

            let object = DataObject::new(
                DataType::from(Module::FileSystem),
                DimensionData::from_buffer(&name, buffer),
            )?;
            cache.add_data(DataType::from(Module::FileSystem), &object)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), file)?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::new_file", &object)
        }

        Ok(())
    }

    async fn to_buffer(&self, name: &str, buffer: &mut Vec<u8>) -> Result<()> {
        let name = affix_root(name);

        if let Ok(cache) = self.get_cache() {
            let name = Path::new(&name)
                .file_name()
                .ok_or(Error::Other)?
                .to_string_lossy()
                .to_string();

            let mut query = QueryBuilder::default();
            query.r#where("name", &name)?;
            if let Ok(list) = cache.get_data(DataType::from(Module::FileSystem), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let obj = list.last().unwrap();
                    if let Ok(data) = obj.payload::<DimensionData>() {
                        data.write_from_path(buffer)?;
                        return Ok(());
                    }
                }
            }
        }

        let _file = self
            .current_directory()
            .get_item(&name[1..])
            .and_then(Item::get_file)?;

        let client = self.client.as_ref();
        match self.client.option {
            IpfsOption::Mfs => {
                let stream = client
                    .files_read(&name)
                    .map_err(|_| std::io::Error::from(ErrorKind::Other));

                let mut reader_stream = StreamReader::new(stream);

                tokio::io::copy(&mut reader_stream, buffer).await?;
            }
            _ => return Err(Error::Unimplemented),
        }

        Ok(())
    }

    async fn remove(&mut self, name: &str, recursive: bool) -> Result<()> {
        let name = affix_root(name);

        //TODO: Resolve to full directory
        if !self.current_directory().has_item(&name[1..]) {
            return Err(warp::error::Error::IoError(std::io::Error::from(
                ErrorKind::NotFound,
            )));
        }
        let client = self.client.as_ref();
        match self.client.option {
            IpfsOption::Mfs => {
                client
                    .files_rm(&name, recursive)
                    .await
                    .map_err(|e| anyhow!(e))?;

                self.current_directory_mut()?.remove_item(&name[1..])?
            }
            _ => return Err(Error::Unimplemented),
        };

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), ())?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::remove_file", &object)
        }
        Ok(())
    }

    async fn create_directory(&mut self, path: &str, recursive: bool) -> Result<()> {
        let path = affix_root(path);

        // check to see if the path exist within the filesystem
        if self.open_directory(&path).is_ok() {
            return Err(Error::Unimplemented);
        }

        match self.client.option {
            IpfsOption::Mfs => {
                let client = self.client.as_ref();
                client
                    .files_mkdir(&path, recursive)
                    .await
                    .map_err(|e| anyhow!(e))?;
            }
            _ => return Err(Error::Unimplemented),
        };

        let directory = Directory::new(&path);

        if let Err(err) = self.current_directory_mut()?.add_item(directory.clone()) {
            let client = self.client.as_ref();
            if let IpfsOption::Mfs = self.client.option {
                client.files_rm(&path, true).await.map_err(|e| anyhow!(e))?;
            };
            return Err(err);
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), directory)?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::create_directory", &object)
        }

        Ok(())
    }

    fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    fn get_path(&self) -> &PathBuf {
        &self.path
    }
}

fn affix_root<S: AsRef<str>>(name: S) -> String {
    let name = String::from(name.as_ref());
    let name = match name.starts_with('/') {
        true => name,
        false => format!("/{}", name),
    };

    name
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    // use crate::IpfsFileSystem;
    // use warp_constellation::constellation::Constellation;

    #[tokio::test]
    async fn default_node_with_buffer() -> Result<()> {
        //TODO: Add a check to determine if ipfs node is running
        // let mut system = IpfsFileSystem::default();
        // system
        //     .from_buffer("test", &b"Hello, World!".to_vec())
        //     .await?;
        //
        // assert_eq!(system.current_directory().has_item("test"), true);
        //
        // let mut buffer: Vec<u8> = vec![];
        //
        // system.to_buffer("test", &mut buffer).await?;
        //
        // assert_eq!(String::from_utf8_lossy(&buffer), "Hello, World!");
        //
        // system.remove("test", false).await?;
        //
        // assert_eq!(system.current_directory().has_item("test"), false);
        Ok(())
    }
}

pub mod ffi {
    use crate::IpfsFileSystem;
    use std::ffi::{c_void, CStr};
    use std::os::raw::c_char;
    use warp::constellation::ConstellationTraitObject;
    use warp::pocket_dimension::PocketDimensionTraitObject;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_fs_ipfs_new(pd: *mut c_void) -> *mut c_void {
        let mut ipfs = IpfsFileSystem::new();

        if pd.is_null() {
            let pd = &*(pd as *mut PocketDimensionTraitObject);
            ipfs.set_cache(pd.inner().clone());
        }

        let obj = Box::new(ConstellationTraitObject::new(Box::new(ipfs)));
        Box::into_raw(obj) as *mut ConstellationTraitObject as *mut c_void
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_fs_ipfs_new_with_uri(
        uri: *const c_char,
        pd: *mut c_void,
    ) -> *mut c_void {
        if uri.is_null() {
            return std::ptr::null_mut();
        }

        let uri = CStr::from_ptr(uri).to_string_lossy().to_string();

        let mut ipfs = match IpfsFileSystem::new_with_uri(uri) {
            Ok(ipfs) => ipfs,
            Err(_) => return std::ptr::null_mut(),
        };

        if pd.is_null() {
            let pd = &*(pd as *mut PocketDimensionTraitObject);
            ipfs.set_cache(pd.inner().clone());
        }

        let obj = Box::new(ConstellationTraitObject::new(Box::new(ipfs)));
        Box::into_raw(obj) as *mut ConstellationTraitObject as *mut c_void
    }
}

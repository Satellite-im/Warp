use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use warp_module::Module;

use warp_common::{
    anyhow,
    chrono::{DateTime, Utc},
    error::Error,
    serde::{Deserialize, Serialize},
    tokio, Extension,
};
use warp_constellation::{constellation::Constellation, directory::Directory};
use warp_pocket_dimension::{query::QueryBuilder, DimensionData, PocketDimension};

use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use s3::BucketConfiguration;
use warp_common::anyhow::bail;

use warp_constellation::item::Item;
use warp_data::{DataObject, DataType};
use warp_hooks::hooks::Hooks;

#[derive(Debug, Clone)]
pub struct StorjClient {
    creds: Option<Credentials>,
    endpoint: String,
}

impl Default for StorjClient {
    fn default() -> Self {
        Self {
            endpoint: String::from("https://gateway.us1.storjshare.io"),
            creds: None,
        }
    }
}

impl StorjClient {
    pub fn new<S: AsRef<str>>(access_key: S, secret_key: S, endpoint: String) -> Self {
        let creds = Some(Credentials {
            access_key: Some(access_key.as_ref().to_string()),
            secret_key: Some(secret_key.as_ref().to_string()),
            security_token: None,
            session_token: None,
        });
        Self { creds, endpoint }
    }

    pub fn set_credentials<S: AsRef<str>>(&mut self, access_key: S, secret_key: S) {
        let creds = Some(Credentials {
            access_key: Some(access_key.as_ref().to_string()),
            secret_key: Some(secret_key.as_ref().to_string()),
            security_token: None,
            session_token: None,
        });
        self.creds = creds;
    }

    pub async fn bucket<S: AsRef<str>>(&self, bucket: S, create: bool) -> anyhow::Result<Bucket> {
        anyhow::ensure!(self.creds.is_some(), warp_common::error::Error::Other);

        let region = Region::Custom {
            region: "us-east-1".to_string(),
            endpoint: self.endpoint.clone(),
        };
        let creds = self.creds.clone().unwrap();
        match create {
            true => {
                let config = BucketConfiguration::default();

                let create_bucket_response =
                    Bucket::create(bucket.as_ref(), region.clone(), creds.clone(), config).await?;
                Ok(create_bucket_response.bucket)
            }
            false => {
                let bucket = Bucket::new(bucket.as_ref(), region, creds)?;
                Ok(bucket)
            }
        }
    }
}

impl Extension for StorjFilesystem {
    fn id(&self) -> String {
        String::from("warp-fs-storj")
    }

    fn name(&self) -> String {
        String::from("Storj Filesystem")
    }

    fn description(&self) -> String {
        String::from("TBD")
    }
    fn module(&self) -> Module {
        Module::FileSystem
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct StorjFilesystem {
    pub index: Directory,
    pub modified: DateTime<Utc>,
    path: PathBuf,
    #[serde(skip)]
    pub client: Option<StorjClient>,
    #[serde(skip)]
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    #[serde(skip)]
    pub hooks: Option<Arc<Mutex<Hooks>>>,
}

impl Default for StorjFilesystem {
    fn default() -> StorjFilesystem {
        StorjFilesystem {
            index: Directory::new("root"),
            path: PathBuf::new(),
            modified: Utc::now(),
            client: None,
            cache: None,
            hooks: None,
        }
    }
}

impl StorjFilesystem {
    pub fn new<S: AsRef<str>>(access_key: S, secret_key: S) -> StorjFilesystem {
        let mut system = StorjFilesystem::default();
        let mut client = StorjClient::default();
        client.set_credentials(access_key, secret_key);
        system.client = Some(client);

        system
    }

    pub fn set_cache(&mut self, cache: Arc<Mutex<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hook: Arc<Mutex<Hooks>>) {
        self.hooks = Some(hook)
    }
}

#[warp_common::async_trait::async_trait]
impl Constellation for StorjFilesystem {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.index
    }

    fn root_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }

    fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    fn get_path(&self) -> &PathBuf {
        &self.path
    }

    fn get_path_mut(&mut self) -> &mut PathBuf {
        &mut self.path
    }

    /// Uploads file from path with the name format being `bucket_name://path/to/file`
    /// Note: This only supports uploading of files. This has not implemented creation of
    ///       directories.
    async fn put(&mut self, name: &str, path: &str) -> warp_common::Result<()> {
        let (bucket, name) = split_for(name)?;

        let mut fs = tokio::fs::File::open(&path).await?;
        let size = fs.metadata().await?.len();

        let client = self.client.as_ref().ok_or(Error::ToBeDetermined)?;
        let code = client
            .bucket(bucket, true)
            .await?
            .put_object_stream(&mut fs, &name)
            .await?;

        if code != 200 {
            return Err(Error::ToBeDetermined);
        }

        let mut file = warp_constellation::file::File::new(&name);
        file.set_size(size as i64);
        file.set_hash(hash_file(path).await?);

        self.open_directory("")?.add_child(file.clone())?;

        self.modified = Utc::now();

        if let Some(cache) = &self.cache {
            let mut cache = cache.lock().unwrap();
            let object = DataObject::new(
                &DataType::Module(Module::FileSystem),
                DimensionData::from_path(path),
            )?;
            cache.add_data(DataType::Module(Module::FileSystem), &object)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(&DataType::Module(Module::FileSystem), file)?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    /// Download file to path with the name format being `bucket_name://path/to/file`
    /// Note: This only supports uploading of files. This has not implemented creation of
    ///       directories.
    async fn get(&self, name: &str, path: &str) -> warp_common::Result<()> {
        let (bucket, name) = split_for(name)?;

        if let Some(cache) = &self.cache {
            let cache = cache.lock().unwrap();
            let mut query = QueryBuilder::default();
            query.r#where("name", &name)?;
            if let Ok(list) = cache.get_data(DataType::Module(Module::FileSystem), Some(&query)) {
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

        let file = self
            .current_directory()
            .get_child(&name)
            .and_then(Item::get_file)?;

        let mut fs = tokio::fs::File::create(path).await?;
        let client = self.client.as_ref().ok_or(Error::ToBeDetermined)?;
        let code = client
            .bucket(bucket, false)
            .await?
            .get_object_stream(&name, &mut fs)
            .await?;

        if code != 200 {
            tokio::fs::remove_file(path).await?;
            return Err(Error::ToBeDetermined);
        }

        let hash = hash_file(path).await?;

        if file.hash != hash {
            tokio::fs::remove_file(path).await?;
            return Err(Error::ToBeDetermined);
        }

        Ok(())
    }

    async fn from_buffer(&mut self, name: &str, buffer: &Vec<u8>) -> warp_common::Result<()> {
        let (bucket, name) = split_for(name)?;

        //TODO: Allow for custom bucket name
        let client = self.client.as_ref().ok_or(Error::Other)?;
        let code = client
            .bucket(bucket, true)
            .await?
            .put_object(&name, buffer)
            .await?;

        if code.1 != 200 {
            return Err(warp_common::error::Error::Other);
        }

        let mut file = warp_constellation::file::File::new(&name);
        file.set_size(buffer.len() as i64);
        file.set_hash(warp_common::hash_data(&buffer));

        self.current_directory_mut()?.add_child(file.clone())?;

        self.modified = Utc::now();

        if let Some(cache) = &self.cache {
            let mut cache = cache.lock().unwrap();
            let object = DataObject::new(
                &DataType::Module(Module::FileSystem),
                DimensionData::from_buffer(name, &buffer),
            )?;
            cache.add_data(DataType::Module(Module::FileSystem), &object)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(&DataType::Module(Module::FileSystem), file)?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    async fn to_buffer(&self, name: &str, buffer: &mut Vec<u8>) -> warp_common::Result<()> {
        let (bucket, name) = split_for(name)?;

        if let Some(cache) = &self.cache {
            let cache = cache.lock().unwrap();
            let mut query = QueryBuilder::default();
            query.r#where("name", &name)?;
            if let Ok(list) = cache.get_data(DataType::Module(Module::FileSystem), Some(&query)) {
                if !list.is_empty() {
                    let obj = list.last().ok_or(Error::ArrayPositionNotFound)?;
                    if let Ok(data) = obj.payload::<DimensionData>() {
                        data.write_from_path(buffer)?;
                        return Ok(());
                    }
                }
            }
        }

        let client = self.client.as_ref().ok_or(Error::ToBeDetermined)?;
        let (buf, code) = client
            .bucket(bucket, false)
            .await?
            .get_object(&name)
            .await?;

        if code != 200 {
            return Err(Error::ToBeDetermined);
        }

        *buffer = buf;

        Ok(())
    }

    async fn remove(&mut self, path: &str, _: bool) -> warp_common::Result<()> {
        let (bucket, name) = split_for(path)?;

        let client = self.client.as_ref().ok_or(Error::Other)?;

        let (_, code) = client
            .bucket(bucket, false)
            .await?
            .delete_object(&name)
            .await?;

        if code != 204 {
            return Err(Error::ToBeDetermined);
        }

        let item = self.current_directory_mut()?.remove_child(&name)?;
        if let Some(hook) = &self.hooks {
            let object = DataObject::new(&DataType::Module(Module::FileSystem), &item)?;
            let hook = hook.lock().unwrap();
            let hook_name = match item {
                Item::Directory(_) => "filesystem::remove_directory",
                Item::File(_) => "filesystem::remove_file",
            };
            hook.trigger(hook_name, &object);
        }
        Ok(())
    }
}

async fn hash_file<S: AsRef<str>>(file: S) -> anyhow::Result<String> {
    let data = tokio::fs::read(file.as_ref()).await?;
    Ok(warp_common::hash_data(data))
}

fn split_for<S: AsRef<str>>(name: S) -> anyhow::Result<(String, String)> {
    let name = name.as_ref();
    let split = if name.contains("://") {
        let name = name.split("://").collect::<Vec<&str>>();

        if name.len() != 2 {
            bail!(Error::InvalidPath);
        }

        (
            name.get(0)
                .ok_or(Error::InvalidConversion)
                .map(|s| s.to_string())?,
            name.get(1)
                .ok_or(Error::InvalidConversion)
                .map(|s| s.to_string())?,
        )
    } else {
        ("root".to_string(), name.to_string())
    };

    Ok(split)
}

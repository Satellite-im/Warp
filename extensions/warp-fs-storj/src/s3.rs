use anyhow::{anyhow, bail};
use aws_endpoint::partition::endpoint;
use aws_endpoint::{CredentialScope, Partition, PartitionResolver};
use aws_sdk_s3::presigning::config::PresigningConfig;
use std::path::PathBuf;
use warp::sata::Sata;
use warp::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use warp::module::Module;

use warp::constellation::{directory::Directory, Constellation};
use warp::pocket_dimension::{query::QueryBuilder, DimensionData, PocketDimension};

use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use s3::BucketConfiguration;

use serde::{Deserialize, Serialize};
use warp::data::{DataObject, DataType};
use warp::hooks::Hooks;
use warp::{Extension, SingleHandle};

use chrono::{DateTime, Utc};
use warp::error::Error;

type Result<T> = std::result::Result<T, warp::error::Error>;

#[derive(Debug, Clone)]
pub struct StorjClient {
    creds: Credentials,
    endpoint: String,
}

impl Default for StorjClient {
    fn default() -> Self {
        Self {
            endpoint: String::from("https://gateway.us1.storjshare.io"),
            creds: Credentials {
                access_key: None,
                secret_key: None,
                security_token: None,
                session_token: None,
            },
        }
    }
}

impl StorjClient {
    pub fn new<S: AsRef<str>>(access_key: S, secret_key: S, endpoint: String) -> Self {
        let creds = Credentials {
            access_key: Some(access_key.as_ref().to_string()),
            secret_key: Some(secret_key.as_ref().to_string()),
            security_token: None,
            session_token: None,
        };
        Self { creds, endpoint }
    }

    pub fn set_credentials<S: AsRef<str>>(&mut self, access_key: S, secret_key: S) {
        let creds = Credentials {
            access_key: Some(access_key.as_ref().to_string()),
            secret_key: Some(secret_key.as_ref().to_string()),
            security_token: None,
            session_token: None,
        };
        self.creds = Credentials {
            access_key: Some(access_key.as_ref().to_string()),
            secret_key: Some(secret_key.as_ref().to_string()),
            ..creds
        }
    }

    pub async fn bucket<S: AsRef<str>>(&self, bucket: S, create: bool) -> anyhow::Result<Bucket> {
        let region = Region::Custom {
            region: "us-east-1".to_string(),
            endpoint: self.endpoint.clone(),
        };
        match create {
            true => {
                let config = BucketConfiguration::default();

                let create_bucket_response =
                    Bucket::create(bucket.as_ref(), region.clone(), self.creds.clone(), config)
                        .await?;
                Ok(create_bucket_response.bucket)
            }
            false => {
                let bucket = Bucket::new(bucket.as_ref(), region, self.creds.clone())?;
                Ok(bucket)
            }
        }
    }

    pub async fn get_or_create_bucket<S: AsRef<str>>(&self, bucket: S) -> anyhow::Result<Bucket> {
        match self.bucket(&bucket, false).await {
            Ok(bucket) => Ok(bucket),
            Err(_) => self.bucket(&bucket, true).await,
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
pub struct StorjFilesystem {
    pub index: Directory,
    pub modified: DateTime<Utc>,
    path: Arc<RwLock<PathBuf>>,
    #[serde(skip)]
    pub client: StorjClient,
    #[serde(skip)]
    pub cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    #[serde(skip)]
    pub hooks: Option<Hooks>,
}

impl Default for StorjFilesystem {
    fn default() -> StorjFilesystem {
        StorjFilesystem {
            index: Directory::new("root"),
            path: Default::default(),
            modified: Utc::now(),
            client: StorjClient::default(),
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
        system.client = client;

        system
    }

    pub fn set_cache(&mut self, cache: Arc<RwLock<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hook: &Hooks) {
        self.hooks = Some(hook.clone())
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

impl SingleHandle for StorjFilesystem {}

#[async_trait::async_trait]
impl Constellation for StorjFilesystem {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> Directory {
        self.index.clone()
    }

    fn set_path(&mut self, path: PathBuf) {
        *self.path.write() = path;
    }

    fn get_path(&self) -> PathBuf {
        self.path.read().clone()
    }

    /// Uploads file from path with the name format being `bucket_name://path/to/file`
    /// Note: This only supports uploading of files. This has not implemented creation of
    ///       directories.
    async fn put(&mut self, name: &str, path: &str) -> Result<()> {
        let (bucket, name) = split_for(name)?;

        let mut fs = tokio::fs::File::open(&path).await?;
        let size = fs.metadata().await?.len();

        let code = self
            .client
            .get_or_create_bucket(&bucket)
            .await?
            .put_object_stream(&mut fs, &name)
            .await
            .map_err(anyhow::Error::from)?;

        if code != 200 {
            //TODO: Do a range check against the code
            return Err(Error::Any(anyhow!(
                "Error uploading file to storj. Code {}",
                code
            )));
        }

        let url = presign_url(
            &self.client.creds.access_key.clone().unwrap_or_default(),
            &self.client.creds.secret_key.clone().unwrap_or_default(),
            &bucket,
            &name,
        )
        .await
        .unwrap_or_default();

        let file = warp::constellation::file::File::new(&name);
        file.set_size(size as usize);
        file.set_reference(&url);
        file.hash_mut().hash_from_file(path)?;

        self.current_directory()?.add_item(file.clone())?;

        self.modified = Utc::now();

        if let Ok(mut cache) = self.get_cache_mut() {
            let object = Sata::default().encode(
                warp::sata::libipld::IpldCodec::DagCbor,
                warp::sata::Kind::Reference,
                DimensionData::from(path),
            )?;
            cache.add_data(DataType::from(Module::FileSystem), &object)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), file)?;
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    /// Download file to path with the name format being `bucket_name://path/to/file`
    /// Note: This only supports uploading of files. This has not implemented creation of
    ///       directories.
    async fn get(&self, name: &str, path: &str) -> Result<()> {
        let (bucket, name) = split_for(name)?;

        if let Ok(cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("name", &name)?;
            if let Ok(list) = cache.get_data(DataType::from(Module::FileSystem), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let obj = list.last().unwrap();
                    if let Ok(data) = obj.decode::<DimensionData>() {
                        if let Ok(mut file) = std::fs::File::create(path) {
                            return data.write_from_path(&mut file);
                        }
                    }
                }
            }
        }

        // let file = self
        //     .current_directory()
        //     .get_item(&name)
        //     .and_then(Item::get_file)?;

        let mut fs = tokio::fs::File::create(path).await?;

        let code = self
            .client
            .bucket(bucket, false)
            .await?
            .get_object_stream(&name, &mut fs)
            .await
            .map_err(anyhow::Error::from)?;

        if code != 200 {
            return match tokio::fs::remove_file(path).await {
                Ok(_) => Err(Error::Any(anyhow!(
                    "Error getting file from storj. Code {}",
                    code
                ))),
                Err(e) => Err(Error::Any(anyhow!(
                    "Error removing file due to code {}: {}",
                    code,
                    e
                ))),
            };
        }

        // TODO:Implement comparing hash
        // let hash = hash_file(path).await?;

        // if file.hash != hash {
        //     tokio::fs::remove_file(path).await?;
        //     return Err(Error::ToBeDetermined);
        // }
        Ok(())
    }

    async fn put_buffer(&mut self, name: &str, buffer: &Vec<u8>) -> Result<()> {
        let (bucket, name) = split_for(name)?;

        //TODO: Allow for custom bucket name
        let code = self
            .client
            .get_or_create_bucket(&bucket)
            .await?
            .put_object(&name, buffer)
            .await
            .map_err(anyhow::Error::from)?;

        if code.1 != 200 {
            return Err(Error::Any(anyhow!(
                "Error uploading file to storj. Code {}",
                code.1
            )));
        }

        let url = presign_url(
            &self.client.creds.access_key.clone().unwrap_or_default(),
            &self.client.creds.secret_key.clone().unwrap_or_default(),
            &bucket,
            &name,
        )
        .await
        .unwrap_or_default();

        self.modified = Utc::now();
        let file = warp::constellation::file::File::new(&name);
        file.set_size(buffer.len());
        file.hash_mut().hash_from_slice(buffer)?;
        file.set_reference(&url);

        self.current_directory()?.add_item(file.clone())?;

        self.modified = Utc::now();

        if let Ok(mut cache) = self.get_cache_mut() {
            let object = Sata::default().encode(
                warp::sata::libipld::IpldCodec::DagCbor,
                warp::sata::Kind::Reference,
                DimensionData::from_buffer(&name, buffer),
            )?;
            cache.add_data(DataType::from(Module::FileSystem), &object)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), file)?;
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    async fn get_buffer(&self, name: &str) -> Result<Vec<u8>> {
        let (bucket, name) = split_for(name)?;

        if let Ok(cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("name", &name)?;
            if let Ok(list) = cache.get_data(DataType::from(Module::FileSystem), Some(&query)) {
                if !list.is_empty() {
                    let obj = list.last().ok_or(Error::ArrayPositionNotFound)?;
                    if let Ok(data) = obj.decode::<DimensionData>() {
                        let mut buffer = vec![];
                        data.write_from_path(&mut buffer)?;
                        return Ok(buffer);
                    }
                }
            }
        }

        let (buf, code) = self
            .client
            .bucket(bucket, false)
            .await?
            .get_object(&name)
            .await
            .map_err(anyhow::Error::from)?;

        if code != 200 {
            return Err(Error::Any(anyhow!(
                "Error getting file from storj. Code {}",
                code
            )));
        }

        Ok(buf)
    }

    async fn remove(&mut self, path: &str, _: bool) -> Result<()> {
        let (bucket, name) = split_for(path)?;

        let (_, code) = self
            .client
            .bucket(bucket, false)
            .await?
            .delete_object(&name)
            .await
            .map_err(anyhow::Error::from)?;

        if code != 204 {
            return Err(Error::Any(anyhow!(
                "Error removing data from storj. Code {}",
                code
            )));
        }

        let item = self.current_directory()?.remove_item(&name)?;
        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::from(Module::FileSystem), &item)?;
            //TODO: Add a proper check
            let hook_name = if item.is_directory() {
                "filesystem::remove_directory"
            } else if item.is_file() {
                "filesystem::remove_file"
            } else {
                "filesystem::unknown_event"
            };
            hook.trigger(hook_name, &object);
        }
        Ok(())
    }

    async fn sync_ref(&mut self, path: &str) -> Result<()> {
        let (bucket, name) = split_for(path)?;

        let url = presign_url(
            &self.client.creds.access_key.clone().unwrap_or_default(),
            &self.client.creds.secret_key.clone().unwrap_or_default(),
            &bucket,
            &name,
        )
        .await
        .unwrap_or_default();

        let dir = self.current_directory()?;
        let file = &mut dir.get_item(&name).and_then(|item| item.get_file())?;
        file.set_reference(&url);
        self.modified = Utc::now();
        Ok(())
    }
}

fn split_for<S: AsRef<str>>(name: S) -> anyhow::Result<(String, String)> {
    let name = name.as_ref();
    let split = if name.contains("://") {
        let name = name.split("://").collect::<Vec<&str>>();

        if name.len() != 2 {
            bail!(Error::InvalidPath);
        }

        (
            name.first()
                .ok_or(Error::InvalidConversion)
                .map(|s| s.to_string())?,
            name.last()
                .ok_or(Error::InvalidConversion)
                .map(|s| s.to_string())?,
        )
    } else {
        ("root".to_string(), name.to_string())
    };

    Ok(split)
}

//TODO: Migrate over to using aws-sdk if we decide to utilize storj s3 over uplink
async fn presign_url(acc: &str, sec: &str, bucket: &str, obj: &str) -> anyhow::Result<String> {
    let cred = aws_sdk_s3::Credentials::new(acc, sec, None, None, "");

    let resolver = PartitionResolver::new(
        Partition::builder()
            .id("storj")
            .region_regex(r#"^(us|eu)\-\w+\-\d+$"#)
            .default_endpoint(endpoint::Metadata {
                uri_template: "gateway.us1.storjshare.io",
                protocol: endpoint::Protocol::Https,
                signature_versions: endpoint::SignatureVersion::V4,
                credential_scope: CredentialScope::builder().build(),
            })
            .build()
            .ok_or_else(|| anyhow!("Error building resolver"))?,
        vec![],
    );
    let config = aws_sdk_s3::config::Config::builder()
        .region(aws_sdk_s3::Region::new("us1"))
        .credentials_provider(cred)
        .endpoint_resolver(resolver)
        .build();

    let client = aws_sdk_s3::Client::from_conf(config);
    let expires_in = std::time::Duration::from_secs(604800);
    let presigned_request = client
        .get_object()
        .bucket(bucket)
        .key(obj)
        .presigned(PresigningConfig::expires_in(expires_in)?)
        .await?;

    Ok(presigned_request.uri().to_string())
}

pub mod ffi {
    use crate::StorjFilesystem;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::constellation::ConstellationAdapter;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::sync::{Arc, RwLock};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_fs_storj_new(
        pd: *const PocketDimensionAdapter,
        akey: *const c_char,
        skey: *const c_char,
    ) -> *mut ConstellationAdapter {
        if akey.is_null() {
            return std::ptr::null_mut();
        }

        if skey.is_null() {
            return std::ptr::null_mut();
        }

        let akey = CStr::from_ptr(akey).to_string_lossy().to_string();

        let skey = CStr::from_ptr(skey).to_string_lossy().to_string();

        let mut client = StorjFilesystem::new(akey, skey);

        if !pd.is_null() {
            let pd = &*pd;
            client.set_cache(pd.inner().clone());
        }

        let obj = Box::new(ConstellationAdapter::new(Arc::new(RwLock::new(Box::new(
            client,
        )))));
        Box::into_raw(obj) as *mut ConstellationAdapter
    }
}

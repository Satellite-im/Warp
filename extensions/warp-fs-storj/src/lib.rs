use aws_endpoint::partition::endpoint;
use aws_endpoint::{CredentialScope, Partition, PartitionResolver};
use aws_sdk_s3::presigning::config::PresigningConfig;
use std::sync::MutexGuard;
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
use warp_constellation::{directory::Directory, Constellation};
use warp_pocket_dimension::{query::QueryBuilder, DimensionData, PocketDimension};

use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use s3::BucketConfiguration;
use warp_common::anyhow::{anyhow, bail};

use warp_constellation::item::Item;
use warp_data::{DataObject, DataType};
use warp_hooks::hooks::Hooks;

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
    pub client: StorjClient,
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

        let code = self
            .client
            .bucket(&bucket, true)
            .await?
            .put_object_stream(&mut fs, &name)
            .await?;

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

        let mut file = warp_constellation::file::File::new(&name);
        file.set_size(size as i64);
        file.set_ref(&url);
        file.hash_mut().hash_from_file(&path)?;

        self.current_directory_mut()?.add_item(file.clone())?;

        self.modified = Utc::now();

        if let Ok(mut cache) = self.get_cache() {
            let object = DataObject::new(
                DataType::Module(Module::FileSystem),
                DimensionData::from_path(path),
            )?;
            cache.add_data(DataType::Module(Module::FileSystem), &object)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::Module(Module::FileSystem), file)?;
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

        if let Ok(cache) = self.get_cache() {
            let mut query = QueryBuilder::default();
            query.r#where("name", &name)?;
            if let Ok(list) = cache.get_data(DataType::Module(Module::FileSystem), Some(&query)) {
                //get last
                if !list.is_empty() {
                    let obj = list.last().unwrap();
                    if let Ok(data) = obj.payload::<DimensionData>() {
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
            .await?;

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

    async fn from_buffer(&mut self, name: &str, buffer: &Vec<u8>) -> warp_common::Result<()> {
        let (bucket, name) = split_for(name)?;

        //TODO: Allow for custom bucket name
        let code = self
            .client
            .bucket(&bucket, true)
            .await?
            .put_object(&name, buffer)
            .await?;

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
        let mut file = warp_constellation::file::File::new(&name);
        file.set_size(buffer.len() as i64);
        file.hash_mut().hash_from_slice(buffer);
        file.set_ref(&url);

        self.current_directory_mut()?.add_item(file.clone())?;

        self.modified = Utc::now();

        if let Ok(mut cache) = self.get_cache() {
            let object = DataObject::new(
                DataType::Module(Module::FileSystem),
                DimensionData::from_buffer(&name, buffer),
            )?;
            cache.add_data(DataType::Module(Module::FileSystem), &object)?;
        }

        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::Module(Module::FileSystem), file)?;
            let hook = hook.lock().unwrap();
            hook.trigger("filesystem::new_file", &object)
        }
        Ok(())
    }

    async fn to_buffer(&self, name: &str, buffer: &mut Vec<u8>) -> warp_common::Result<()> {
        let (bucket, name) = split_for(name)?;

        if let Ok(cache) = self.get_cache() {
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

        let (buf, code) = self
            .client
            .bucket(bucket, false)
            .await?
            .get_object(&name)
            .await?;

        if code != 200 {
            return Err(Error::Any(anyhow!(
                "Error getting file from storj. Code {}",
                code
            )));
        }

        *buffer = buf;

        Ok(())
    }

    async fn remove(&mut self, path: &str, _: bool) -> warp_common::Result<()> {
        let (bucket, name) = split_for(path)?;

        let (_, code) = self
            .client
            .bucket(bucket, false)
            .await?
            .delete_object(&name)
            .await?;

        if code != 204 {
            return Err(Error::Any(anyhow!(
                "Error removing data from storj. Code {}",
                code
            )));
        }

        let item = self.current_directory_mut()?.remove_item(&name)?;
        if let Some(hook) = &self.hooks {
            let object = DataObject::new(DataType::Module(Module::FileSystem), &item)?;
            let hook = hook.lock().unwrap();
            let hook_name = match item {
                Item::Directory(_) => "filesystem::remove_directory",
                Item::File(_) => "filesystem::remove_file",
            };
            hook.trigger(hook_name, &object);
        }
        Ok(())
    }

    async fn sync_ref(&mut self, path: &str) -> warp_common::Result<()> {
        let (bucket, name) = split_for(path)?;

        let url = presign_url(
            &self.client.creds.access_key.clone().unwrap_or_default(),
            &self.client.creds.secret_key.clone().unwrap_or_default(),
            &bucket,
            &name,
        )
        .await
        .unwrap_or_default();

        let dir = self.current_directory_mut()?;
        let file = dir.get_item_mut(&name).and_then(Item::get_file_mut)?;
        file.set_ref(&url);
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

//TODO: Migrate over to using aws-sdk if we decide to utilize storj s3 over uplink
async fn presign_url(
    acc: &str,
    sec: &str,
    bucket: &str,
    obj: &str,
) -> warp_common::anyhow::Result<String> {
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

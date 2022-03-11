

use std::sync::{Arc, Mutex};


use warp_pocket_dimension::PocketDimension;
use warp_common::chrono::{DateTime, Utc};
use warp_common::error::Error;
use warp_common::anyhow;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::Extension;
use warp_constellation::constellation::{Constellation, ConstellationVersion};
use warp_constellation::directory::Directory;
use warp_constellation::file::File;
use warp_data::DataObject;
use warp_module::Module;
use warp_pocket_dimension::query::QueryBuilder;
use warp_common::tokio;


use s3::BucketConfiguration;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;

#[derive(Debug, Clone)]
pub struct StorjClient {
    creds: Option<Credentials>,
    endpoint: String,
}

impl Default for StorjClient {
    fn default() -> Self {
        Self {
            endpoint: String::from("https://gateway.us1.storjshare.io"),
            ..Default::default()
        }
    }
}

impl StorjClient {

    pub fn new<S: AsRef<str>>(access_key: S, secret_key: S, endpoint: String) -> Self {
        let creds = Some(Credentials{access_key: Some(access_key.as_ref().to_string()), secret_key: Some(secret_key.as_ref().to_string()), security_token: None, session_token: None });
        Self { creds, endpoint }
    }

    pub fn set_credentials<S: AsRef<str>>(&mut self, access_key: S, secret_key: S) {
        let creds = Some(Credentials{access_key: Some(access_key.as_ref().to_string()), secret_key: Some(secret_key.as_ref().to_string()), security_token: None, session_token: None });
        self.creds = creds;
    }

    pub async fn bucket<S: AsRef<str>>(&self, bucket: S, create: bool) -> anyhow::Result<Bucket> {
        
        anyhow::ensure!(self.creds.is_some(), warp_common::error::Error::Other);

        let region = Region::Custom { region: "us-east-1".to_string(), endpoint: self.endpoint.clone() };
        let creds = self.creds.clone().unwrap();
        match create {
            true => {
                let config = BucketConfiguration::default();
                let create_bucket_response = Bucket::create(bucket.as_ref(), region.clone(), creds.clone(), config).await?;
                return Ok(create_bucket_response.bucket);
            },
            false => {
                let bucket = Bucket::new(bucket.as_ref(), region, creds)?;
                Ok(bucket)
            }
        }
        
    }

}

impl Extension for StorjFilesystem {
    fn id(&self) -> String { format!("warp-fs-storj") }

    fn name(&self) -> String {
        String::from("Storj Filesystem")
    }

    fn description(&self) -> String {
        String::from("TBD")
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct StorjFilesystem {
    pub version: ConstellationVersion,
    pub index: Directory,
    pub modified: DateTime<Utc>,
    #[serde(skip)]
    pub client: StorjClient,
    #[serde(skip)]
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>
}

impl Default for StorjFilesystem {
    fn default() -> Self {
        StorjFilesystem {
            version: ConstellationVersion::from((0, 1, 0)),
            index: Directory::new("root"),
            modified: Utc::now(),
            ..Default::default()
        }
    }
}

impl StorjFilesystem {

    pub fn set_cache(&mut self, cache: impl PocketDimension + 'static) {
        self.cache = Some(Arc::new(Mutex::new(Box::new(cache))));
    }

}

#[warp_common::async_trait::async_trait]
impl Constellation for StorjFilesystem {
    fn version(&self) -> &ConstellationVersion {
        &self.version
    }

    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.index
    }

    fn root_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }

    async fn put(
        &mut self,
        name: &str,
        path: &str,
    ) -> warp_common::Result<()> {
        let mut fs = tokio::fs::File::open(path).await?;

        //TODO: Allow for custom bucket name
        let _code = self.client.bucket("root", true).await?.put_object_stream(&mut fs, name).await?;

        Ok(())
    }

    async fn get(
        &self,
        name: &str,
        _path: &str,
    ) -> warp_common::Result<()> {
        
        if !self.root_directory().has_child(name) {
            return Err(warp_common::error::Error::Other);
        }

        if let Some(cache) = &self.cache {
            let cache = cache.lock().unwrap();
            let mut query = QueryBuilder::default();
            query.r#where("name", name.to_string())?;
            match cache.get_data(Module::FileSystem, Some(&query)) {
                Ok(d) => {
                    //get last
                    if !d.is_empty() {
                        let mut list = d.clone();
                        let _obj = list.pop().unwrap();
                        
                        //TODO:
                        return Ok(());
                    }
                }
                Err(_) => {}
            }
        }

        Ok(())
    }
}

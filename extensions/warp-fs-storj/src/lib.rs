use blake2::{Blake2b512, Digest};
use std::sync::{Arc, Mutex};

use warp_common::{
    anyhow,
    chrono::{DateTime, Utc},
    error::Error,
    serde::{Deserialize, Serialize},
    tokio, Extension,
};
use warp_constellation::{constellation::Constellation, directory::Directory};
use warp_pocket_dimension::PocketDimension;

use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use s3::BucketConfiguration;

use warp_constellation::item::Item;

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
                return Ok(create_bucket_response.bucket);
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
        format!("warp-fs-storj")
    }

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
    pub index: Directory,
    pub modified: DateTime<Utc>,
    #[serde(skip)]
    pub client: Option<StorjClient>,
    #[serde(skip)]
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
}

impl Default for StorjFilesystem {
    fn default() -> StorjFilesystem {
        StorjFilesystem {
            index: Directory::new("root"),
            modified: Utc::now(),
            client: None,
            cache: None,
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

    pub fn set_cache(&mut self, cache: impl PocketDimension + 'static) {
        self.cache = Some(Arc::new(Mutex::new(Box::new(cache))));
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

    /// Uploads file from path with the name format being `bucket_name://path/to/file`
    /// Note: This only supports uploading of files. This has not implemented creation of
    ///       directories.
    /// TODO: Implement cache
    async fn put(&mut self, name: &str, path: &str) -> warp_common::Result<()> {
        let name = name.split("://").collect::<Vec<&str>>();

        if name.len() != 2 {
            return Err(Error::ToBeDetermined);
        }

        let (bucket, in_name) = (
            name.get(0).ok_or(Error::ToBeDetermined)?,
            name.get(1).ok_or(Error::ToBeDetermined)?,
        );

        let mut fs = tokio::fs::File::open(path).await?;
        let size = fs.metadata().await?.len();

        let client = self.client.as_ref().ok_or(Error::ToBeDetermined)?;
        let code = client
            .bucket(bucket, true)
            .await?
            .put_object_stream(&mut fs, in_name)
            .await?;

        if code != 200 {
            return Err(Error::ToBeDetermined);
        }

        let mut file = warp_constellation::file::File::new(in_name);
        file.set_size(size as i64);
        file.set_hash(hash_file(path).await?);

        self.open_directory("")?.add_child(file)?;

        self.modified = Utc::now();

        //TODO: Deal with caching that can be more adaptive any ext of pd
        //Note: Since caching extensions may have different methods of handling
        //      and storing of data, we would need to look into a streamline
        //      way of providing data to cache without buffering large amount
        //      of data to memory.

        Ok(())
    }

    /// Download file to path with the name format being `bucket_name://path/to/file`
    /// Note: This only supports uploading of files. This has not implemented creation of
    ///       directories.
    /// TODO: Implement cache
    async fn get(&self, name: &str, path: &str) -> warp_common::Result<()> {
        let name = name.split("://").collect::<Vec<&str>>();

        if name.len() != 2 {
            return Err(Error::ToBeDetermined);
        }

        let (bucket, out_name) = (
            name.get(0).ok_or(Error::ToBeDetermined)?,
            name.get(1).ok_or(Error::ToBeDetermined)?,
        );

        //TODO: Implement cache
        let file = self
            .root_directory()
            .get_child_by_path(out_name)
            .and_then(Item::get_file)?;

        let mut fs = tokio::fs::File::create(path).await?;
        let client = self.client.as_ref().ok_or(Error::ToBeDetermined)?;
        let code = client
            .bucket(bucket, false)
            .await?
            .get_object_stream(out_name, &mut fs)
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
        let name = name.split("://").collect::<Vec<&str>>();

        if name.len() != 2 {
            return Err(Error::ToBeDetermined);
        }

        let (bucket, in_name) = (
            name.get(0).ok_or(Error::ToBeDetermined)?,
            name.get(1).ok_or(Error::ToBeDetermined)?,
        );

        //TODO: Allow for custom bucket name
        let client = self.client.as_ref().ok_or(Error::Other)?;
        let code = client
            .bucket(bucket, true)
            .await?
            .put_object(in_name, &buffer)
            .await?;

        if code.1 != 200 {
            return Err(warp_common::error::Error::Other);
        }

        let mut file = warp_constellation::file::File::new(in_name);
        file.set_size(buffer.len() as i64);
        file.set_hash(hash_data(buffer));

        self.open_directory("")?.add_child(file)?;
        //TODO: Mutate/update the modified date.

        //TODO: Deal with caching that can be more adaptive any ext of pd
        //Note: Since caching extensions may have different methods of handling
        //      and storing of data, we would need to look into a streamline
        //      way of providing data to cache without buffering large amount
        //      of data to memory.

        Ok(())
    }

    async fn to_buffer(&self, name: &str, buffer: &mut Vec<u8>) -> warp_common::Result<()> {
        let name = name.split("://").collect::<Vec<&str>>();

        if name.len() != 2 {
            return Err(Error::ToBeDetermined);
        }

        let (bucket, out_name) = (
            name.get(0).ok_or(Error::ToBeDetermined)?,
            name.get(1).ok_or(Error::ToBeDetermined)?,
        );

        //TODO: Implement cache

        let file = self
            .root_directory()
            .get_child_by_path(out_name)
            .and_then(Item::get_file)?;

        let client = self.client.as_ref().ok_or(Error::ToBeDetermined)?;
        let code = client
            .bucket(bucket, false)
            .await?
            .get_object(out_name)
            .await?;

        if code.1 != 200 {
            return Err(Error::ToBeDetermined);
        }

        let hash = hash_data(&code.0);

        if file.hash != hash {
            return Err(Error::ToBeDetermined);
        }

        *buffer = code.0;

        Ok(())
    }

    async fn remove(&mut self, path: &str) -> warp_common::Result<()> {
        let name = path.split("://").collect::<Vec<&str>>();

        if name.len() != 2 {
            return Err(Error::ToBeDetermined);
        }

        let (bucket, name) = (
            name.get(0).ok_or(Error::ToBeDetermined)?,
            name.get(1).ok_or(Error::ToBeDetermined)?,
        );

        let client = self.client.as_ref().ok_or(Error::Other)?;

        let code = client
            .bucket(bucket, false)
            .await?
            .delete_object(name)
            .await?;

        if code.1 != 204 {
            return Err(Error::ToBeDetermined);
        }

        self.root_directory_mut().remove_child(name)?;

        Ok(())
    }
}

async fn hash_file<S: AsRef<str>>(file: S) -> anyhow::Result<String> {
    let data = tokio::fs::read(file.as_ref()).await?;
    Ok(hash_data(data))
}

fn hash_data<S: AsRef<[u8]>>(data: S) -> String {
    let mut hasher = Blake2b512::new();
    hasher.update(&data.as_ref());
    let res = hasher.finalize().to_vec();
    hex::encode(res)
}

use std::{collections::HashSet, ffi::OsStr, path::PathBuf, sync::Arc};

use chrono::{DateTime, Utc};
use futures::{
    pin_mut,
    stream::{self, BoxStream},
    StreamExt, TryStreamExt,
};
use libipld::Cid;
use rust_ipfs::{
    unixfs::{AddOpt, AddOption, UnixfsStatus},
    Ipfs, IpfsPath,
};
use tokio::sync::broadcast;
use tokio_util::io::ReaderStream;
use warp::{
    constellation::{
        directory::Directory, ConstellationEventKind, ConstellationProgressStream, Progression,
    },
    error::Error,
    sync::RwLock,
};

use crate::{
    config::{self, Config},
    thumbnail::ThumbnailGenerator,
    to_file_type,
};

use super::{ecdh_decrypt, ecdh_encrypt, get_keypair_did};

#[derive(Clone)]
#[allow(dead_code)]
pub struct FileStore {
    index: Directory,
    path: Arc<RwLock<PathBuf>>,
    modified: DateTime<Utc>,
    index_cid: Arc<RwLock<Option<Cid>>>,
    thumbnail_store: ThumbnailGenerator,

    location_path: Option<PathBuf>,

    ipfs: Ipfs,
    constellation_tx: broadcast::Sender<ConstellationEventKind>,

    config: config::Config,
}

impl FileStore {
    pub async fn new(
        ipfs: Ipfs,
        config: &Config,
        constellation_tx: broadcast::Sender<ConstellationEventKind>,
    ) -> Result<Self, Error> {
        let mut index_cid = None;

        if let Some(path) = config.path.as_ref() {
            if !path.exists() {
                tokio::fs::create_dir_all(path).await?;
            }

            if let Ok(cid) = tokio::fs::read(path.join(".index_id"))
                .await
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .and_then(|cid_str| {
                    cid_str
                        .parse::<Cid>()
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                })
            {
                index_cid = Some(cid);
            }
        }

        let config = config.clone();

        let index = Directory::new("root");
        let path = Arc::default();
        let modified = Utc::now();
        let index_cid = Arc::new(RwLock::new(index_cid));
        let thumbnail_store = ThumbnailGenerator::default();

        let location_path = config.path.clone();

        let store = FileStore {
            index,
            path,
            modified,
            index_cid,
            thumbnail_store,
            location_path,
            ipfs,
            constellation_tx,
            config,
        };

        if let Err(e) = store.import().await {
            tracing::error!("Error importing index: {e}");
        }

        tokio::spawn({
            let fs = store.clone();
            async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    if let Err(_e) = fs.export().await {
                        tracing::error!("Error exporting index: {_e}");
                    }
                }
            }
        });

        Ok(store)
    }

    async fn import(&self) -> Result<(), Error> {
        if self.config.path.is_none() {
            return Ok(());
        }

        let cid = (*self.index_cid.read()).ok_or(Error::Other)?;

        let mut index_stream = self
            .ipfs
            .unixfs()
            .cat(IpfsPath::from(cid), None, &[], true)
            .await
            .map_err(anyhow::Error::from)?
            .boxed();

        let mut data = vec![];

        while let Some(bytes) = index_stream.try_next().await.map_err(anyhow::Error::from)? {
            data.extend(bytes);
        }

        let key = self.ipfs.keypair().and_then(get_keypair_did)?;

        let index_bytes = ecdh_decrypt(&key, None, data)?;

        let mut directory_index: Directory = serde_json::from_slice(&index_bytes)?;
        directory_index.rebuild_paths();

        self.index.set_items(directory_index.get_items());

        Ok(())
    }

    async fn export(&self) -> Result<(), Error> {
        let index = serde_json::to_string(&self.index)?;

        let key = self.ipfs.keypair().and_then(get_keypair_did)?;
        let data = ecdh_encrypt(&key, None, index.as_bytes())?;

        let data_stream = stream::once(async move { Ok::<_, std::io::Error>(data) }).boxed();

        let mut stream = self
            .ipfs
            .unixfs()
            .add(
                AddOpt::Stream(data_stream),
                Some(AddOption {
                    pin: true,
                    ..Default::default()
                }),
            )
            .await?;

        let mut ipfs_path = None;

        while let Some(status) = stream.next().await {
            match status {
                UnixfsStatus::CompletedStatus { path, .. } => ipfs_path = Some(path),
                UnixfsStatus::FailedStatus { error, .. } => {
                    return match error {
                        Some(e) => Err(Error::Any(e)),
                        None => Err(Error::Other),
                    }
                }
                _ => {}
            }
        }

        let ipfs_path = ipfs_path.ok_or(Error::OtherWithContext("Ipfs path unavailable".into()))?;

        let cid = ipfs_path
            .root()
            .cid()
            .ok_or(Error::OtherWithContext("unable to get cid".into()))?;

        let last_cid = { *self.index_cid.read() };

        *self.index_cid.write() = Some(*cid);

        if let Some(last_cid) = last_cid {
            if *cid != last_cid {
                let mut pinned_blocks: HashSet<_> = HashSet::from_iter(
                    self.ipfs
                        .list_pins(None)
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

                if self.ipfs.is_pinned(&last_cid).await? {
                    self.ipfs.remove_pin(&last_cid, true).await?;
                }

                let new_pinned_blocks: HashSet<_> = HashSet::from_iter(
                    self.ipfs
                        .list_pins(None)
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
                    self.ipfs.remove_block(cid).await?;
                }
            }
        }

        if let Some(path) = self.config.path.as_ref() {
            if let Err(_e) = tokio::fs::write(path.join(".index_id"), cid.to_string()).await {
                tracing::error!("Error writing index: {_e}");
            }
        }
        Ok(())
    }
}

impl FileStore {
    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    pub fn root_directory(&self) -> Directory {
        self.index.clone()
    }

    /// Get the current directory that is mutable.
    pub fn current_directory(&self) -> Result<Directory, Error> {
        self.open_directory(&self.get_path().to_string_lossy())
    }

    /// Returns a mutable directory from the filesystem
    pub fn open_directory(&self, path: &str) -> Result<Directory, Error> {
        match path.trim().is_empty() {
            true => Ok(self.root_directory()),
            false => self
                .root_directory()
                .get_item_by_path(path)
                .and_then(|item| item.get_directory()),
        }
    }

    /// Current size of the file system
    pub fn current_size(&self) -> usize {
        self.root_directory().size()
    }

    pub fn max_size(&self) -> usize {
        self.config.max_storage_size.unwrap_or(1024 * 1024 * 1024)
    }

    pub async fn put(
        &mut self,
        name: &str,
        path: &str,
    ) -> Result<ConstellationProgressStream, Error> {
        let name = name.trim();
        if name.len() < 2 || name.len() > 256 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name.len(),
                minimum: Some(2),
                maximum: Some(256),
            });
        }

        let ipfs = self.ipfs.clone();

        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(Error::FileNotFound);
        }

        let file_size = tokio::fs::metadata(&path).await?.len();

        if self.current_size() + (file_size as usize) >= self.max_size() {
            return Err(Error::InvalidLength {
                context: path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .map(str::to_string)
                    .unwrap_or("path".to_string()),
                current: self.current_size() + file_size as usize,
                minimum: None,
                maximum: Some(self.max_size()),
            });
        }

        let current_directory = self.current_directory()?;

        if current_directory.get_item_by_path(name).is_ok() {
            return Err(Error::FileExist);
        }

        let ((width, height), exact) = (
            self.config.thumbnail_size,
            self.config.thumbnail_exact_format,
        );

        let ticket = self
            .thumbnail_store
            .insert(&path, width, height, exact)
            .await?;

        let background = self.config.thumbnail_task;

        let name = name.to_string();
        let fs = self.clone();
        let progress_stream = async_stream::stream! {

            let mut last_written = 0;

            let mut total_written = 0;
            let mut returned_path = None;

            let mut stream = match ipfs.unixfs().add(path.clone(), Some(AddOption { pin: true, ..Default::default() })).await {
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
                    UnixfsStatus::CompletedStatus { path, written, total_size } => {
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
                    }
                    UnixfsStatus::ProgressStatus { written, total_size } => {
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

            let file = warp::constellation::file::File::new(&name);
            file.set_size(total_written);
            file.set_reference(&format!("{ipfs_path}"));
            file.set_file_type(to_file_type(&name));

            if let Ok(Err(_e)) = tokio::task::spawn_blocking({
                let f = file.clone();
                move || f.hash_mut().hash_from_file(&path)
            }).await {}

            if let Err(e) = current_directory.add_item(file.clone()) {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: Some(e.to_string()),
                };
                return;
            }

            let task = {
                let fs = fs.clone();
                let store = fs.thumbnail_store.clone();
                let file = file.clone();
                async move {
                    match store.get(ticket).await {
                        Ok((extension_type, thumbnail)) => {
                            file.set_thumbnail(&thumbnail);
                            file.set_thumbnail_format(extension_type.into());
                            //We export again so the thumbnail can be apart of the index
                            if background {
                                if let Err(_e) = fs.export().await {}
                            }
                        }
                        Err(_e) => {}
                    }
                }
            };

            if !background {
                task.await;
                if let Err(_e) = fs.export().await {}
            } else {
                tokio::spawn(task);
            }

            yield Progression::ProgressComplete {
                name: name.to_string(),
                total: Some(total_written),
            };

            let _ = fs.constellation_tx.send(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written)
            }).ok();
        };

        Ok(ConstellationProgressStream(progress_stream.boxed()))
    }

    pub async fn get(&self, name: &str, path: &str) -> Result<(), Error> {
        let ipfs = self.ipfs.clone();

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
        if let Err(_e) = self
            .constellation_tx
            .send(ConstellationEventKind::Downloaded {
                filename: file.name(),
                size: Some(file.size()),
                location: Some(PathBuf::from(path)),
            })
        {}
        Ok(())
    }

    pub async fn put_buffer(&mut self, name: &str, buffer: &[u8]) -> Result<(), Error> {
        let name = name.trim();
        if name.len() < 2 || name.len() > 256 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name.len(),
                minimum: Some(2),
                maximum: Some(256),
            });
        }

        if self.current_size() + buffer.len() >= self.max_size() {
            return Err(Error::InvalidLength {
                context: "buffer".into(),
                current: self.current_size() + buffer.len(),
                minimum: None,
                maximum: Some(self.max_size()),
            });
        }

        let ipfs = self.ipfs.clone();

        if self.current_directory()?.get_item_by_path(name).is_ok() {
            return Err(Error::FileExist);
        }

        let ((width, height), exact) = (
            self.config.thumbnail_size,
            self.config.thumbnail_exact_format,
        );

        let ticket = self
            .thumbnail_store
            .insert_buffer(&name, buffer, width, height, exact)
            .await;

        let reader = ReaderStream::new(std::io::Cursor::new(buffer))
            .map(|result| result.map(|x| x.into()))
            .boxed();

        let mut total_written = 0;
        let mut returned_path = None;

        let mut stream = ipfs
            .unixfs()
            .add(
                reader,
                Some(AddOption {
                    pin: true,
                    ..Default::default()
                }),
            )
            .await?;

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

        let file = warp::constellation::file::File::new(name);
        file.set_size(total_written);
        file.set_reference(&format!("{ipfs_path}"));
        file.set_file_type(to_file_type(name));
        file.hash_mut().hash_from_slice(buffer)?;

        match self.thumbnail_store.get(ticket).await {
            Ok((extension_type, thumbnail)) => {
                file.set_thumbnail(&thumbnail);
                file.set_thumbnail_format(extension_type.into())
            }
            Err(_e) => {}
        }

        self.current_directory()?.add_item(file)?;

        if let Err(_e) = self.export().await {}
        let _ = self
            .constellation_tx
            .send(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written),
            })
            .ok();
        Ok(())
    }

    pub async fn get_buffer(&self, name: &str) -> Result<Vec<u8>, Error> {
        let ipfs = self.ipfs.clone();

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
            .constellation_tx
            .send(ConstellationEventKind::Downloaded {
                filename: file.name(),
                size: Some(file.size()),
                location: None,
            })
            .ok();
        Ok(buffer)
    }

    /// Used to upload file to the filesystem with data from a stream
    pub async fn put_stream(
        &mut self,
        name: &str,
        total_size: Option<usize>,
        stream: BoxStream<'static, Vec<u8>>,
    ) -> Result<ConstellationProgressStream, Error> {
        let name = name.trim();
        if name.len() < 2 || name.len() > 256 {
            return Err(Error::InvalidLength {
                context: "name".into(),
                current: name.len(),
                minimum: Some(2),
                maximum: Some(256),
            });
        }

        let ipfs = self.ipfs.clone();
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

            let mut stream = match ipfs.unixfs().add(stream, Some(AddOption { pin: true, ..Default::default() })).await {
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
                let n = name.clone();
                match status {
                    UnixfsStatus::CompletedStatus { path, written, .. } => {
                        returned_path = Some(path);
                        total_written = written;
                        last_written = written;
                        yield Progression::CurrentProgress {
                            name: n,
                            current: written,
                            total: total_size,
                        };
                    }
                    UnixfsStatus::FailedStatus {
                        written, error, ..
                    } => {
                        last_written = written;
                        yield Progression::ProgressFailed {
                            name: n,
                            last_size: Some(last_written),
                            error: error.map(|e| e.to_string()),
                        };
                        return;
                    }
                    UnixfsStatus::ProgressStatus { written, .. } => {
                        last_written = written;
                        yield Progression::CurrentProgress {
                            name: n,
                            current: written,
                            total: total_size,
                        };
                    }
                }

                if fs.current_size() + last_written >= fs.max_size() {
                    yield Progression::ProgressFailed {
                        name,
                        last_size: Some(last_written),
                        error: Some(Error::InvalidLength {
                            context: "buffer".into(),
                            current: fs.current_size() + last_written,
                            minimum: None,
                            maximum: Some(fs.max_size()),
                        }).map(|e| e.to_string())
                    };
                    return;
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

            let file = warp::constellation::file::File::new(&name);
            file.set_size(total_written);
            file.set_reference(&format!("{ipfs_path}"));
            file.set_file_type(to_file_type(&name));
            // file.hash_mut().hash_from_slice(buffer)?;
            if let Err(e) = current_directory.add_item(file) {
                yield Progression::ProgressFailed {
                    name,
                    last_size: Some(last_written),
                    error: Some(e.to_string()),
                };
                return;
            }

            if let Err(_e) = fs.export().await {}

            yield Progression::ProgressComplete {
                name: name.to_string(),
                total: Some(total_written),
            };

            let _ = fs.constellation_tx.send(ConstellationEventKind::Uploaded {
                filename: name.to_string(),
                size: Some(total_written)
            }).ok();
        };

        Ok(ConstellationProgressStream(progress_stream.boxed()))
    }

    /// Used to download data from the filesystem using a stream
    pub async fn get_stream(
        &self,
        name: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        let ipfs = self.ipfs.clone();
        //Used to enter the tokio context
        let handle = warp::async_handle();
        let _g = handle.enter();

        let item = self.current_directory()?.get_item_by_path(name)?;
        let file = item.get_file()?;
        let size = file.size();
        let reference = file.reference().ok_or(Error::Other)?; //Reference not found

        let tx = self.constellation_tx.clone();

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
    pub async fn remove(&mut self, name: &str, _: bool) -> Result<(), Error> {
        let ipfs = self.ipfs.clone();
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
        if let Err(_e) = self.export().await {}

        let _ = self
            .constellation_tx
            .send(ConstellationEventKind::Deleted {
                item_name: name.to_string(),
            })
            .ok();

        Ok(())
    }

    pub async fn rename(&mut self, current: &str, new: &str) -> Result<(), Error> {
        //Used as guard in the event its not available but will be used in the future
        let _ipfs = self.ipfs.clone();

        //Note: This will only support renaming the file or directory in the index
        let directory = self.current_directory()?;

        if directory.get_item_by_path(new).is_ok() {
            return Err(Error::DuplicateName);
        }

        directory.rename_item(current, new)?;
        if let Err(_e) = self.export().await {}
        let _ = self
            .constellation_tx
            .send(ConstellationEventKind::Renamed {
                old_item_name: current.to_string(),
                new_item_name: new.to_string(),
            })
            .ok();
        Ok(())
    }

    pub async fn create_directory(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        //Used as guard in the event its not available but will be used in the future
        let _ipfs = self.ipfs.clone();

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
        if let Err(_e) = self.export().await {}
        Ok(())
    }

    pub async fn sync_ref(&mut self, path: &str) -> Result<(), Error> {
        let ipfs = self.ipfs.clone();

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

        let mut buffer = vec![];
        while let Some(data) = stream.next().await {
            let bytes = data.map_err(anyhow::Error::from)?;
            buffer.extend(bytes);
        }

        let ((width, height), exact) = (
            self.config.thumbnail_size,
            self.config.thumbnail_exact_format,
        );

        // Generate the thumbnail for the file
        let id = self
            .thumbnail_store
            .insert_buffer(file.name(), &buffer, width, height, exact)
            .await;

        if let Ok((extension_type, thumbnail)) = self.thumbnail_store.get(id).await {
            file.set_thumbnail(&thumbnail);
            file.set_thumbnail_format(extension_type.into())
        }

        let _ = self.export().await;

        Ok(())
    }

    pub fn set_path(&mut self, path: PathBuf) {
        *self.path.write() = path;
    }

    pub fn get_path(&self) -> PathBuf {
        PathBuf::from(self.path.read().to_string_lossy().replace('\\', "/"))
    }
}

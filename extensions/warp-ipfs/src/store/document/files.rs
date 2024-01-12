use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::stream::FuturesUnordered;
use futures::{StreamExt, TryFutureExt};
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use serde::{Deserialize, Serialize};
use warp::constellation::item::Item;
use warp::error::Error;

use warp::constellation::{
    directory::Directory,
    file::{File, FileType, Hash},
};

use crate::store::document::image_dag::ImageDag;

#[derive(Clone, Deserialize, Serialize)]
pub struct DirectoryDocument {
    pub name: String,
    pub description: String,
    pub favorite: bool,
    pub creation: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub thumbnail: Option<Cid>,
    pub items: Option<Cid>,
}

impl DirectoryDocument {
    #[async_recursion::async_recursion]
    pub async fn new(ipfs: &Ipfs, root: &Directory) -> Result<DirectoryDocument, Error> {
        let mut document = DirectoryDocument {
            name: root.name(),
            description: root.description(),
            favorite: root.favorite(),
            creation: root.creation(),
            modified: root.modified(),
            thumbnail: None,
            items: None,
        };

        let items = FuturesUnordered::from_iter(
            root.get_items()
                .iter()
                .map(|item| ItemDocument::new(ipfs, item).into_future()),
        )
        .filter_map(|result| async { result.ok() })
        .collect::<Vec<_>>()
        .await;

        let cid = ipfs.dag().put().serialize(items)?.await?;

        document.items = Some(cid);

        if let Some(cid) = root
            .thumbnail_reference()
            .and_then(|refs| refs.parse::<IpfsPath>().ok())
            .and_then(|path| path.root().cid().copied())
        {
            document.thumbnail = ipfs
                .repo()
                .contains(&cid)
                .await
                .unwrap_or_default()
                .then_some(cid)
        }

        Ok(document)
    }

    #[async_recursion::async_recursion]
    pub async fn resolve(&self, ipfs: &Ipfs) -> Result<Directory, Error> {
        let mut directory = Directory::new(&self.name);
        directory.set_description(&self.description);
        directory.set_favorite(self.favorite);
        directory.set_creation(self.creation);
        directory.set_modified(Some(self.modified));

        if let Some(cid) = self.items {
            let list = ipfs
                .get_dag(cid)
                .timeout(Duration::from_secs(10))
                .deserialized::<Vec<ItemDocument>>()
                .await
                .unwrap_or_default();

            let items_resolved = FuturesUnordered::from_iter(
                list.iter().map(|item| item.resolve(ipfs).into_future()),
            )
            .filter_map(|item| async { item.ok() });

            futures::pin_mut!(items_resolved);

            while let Some(item) = items_resolved.next().await {
                _ = directory.add_item(item);
            }
        }

        if let Some(cid) = self.thumbnail {
            let image: ImageDag = ipfs
                .get_dag(cid)
                .timeout(Duration::from_secs(10))
                .deserialized()
                .await?;

            directory.set_thumbnail_format(image.mime.into());

            let data = ipfs
                .unixfs()
                .cat(image.link, None, &[], false, Some(Duration::from_secs(10)))
                .await
                .unwrap_or_default();

            directory.set_thumbnail(&data);
        }

        directory.rebuild_paths(&None);
        Ok(directory)
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ItemDocument {
    Directory(Cid),
    File(Cid),
}

impl ItemDocument {
    pub async fn new(ipfs: &Ipfs, item: &Item) -> Result<ItemDocument, Error> {
        let document = match item {
            Item::File(file) => {
                let document = FileDocument::new(ipfs, file).await?;
                let cid = ipfs.dag().put().serialize(document)?.await?;
                ItemDocument::File(cid)
            }
            Item::Directory(directory) => {
                let document = DirectoryDocument::new(ipfs, directory).await?;
                let cid = ipfs.dag().put().serialize(document)?.await?;
                ItemDocument::Directory(cid)
            }
        };

        Ok(document)
    }

    pub async fn resolve(&self, ipfs: &Ipfs) -> Result<Item, Error> {
        let item = match *self {
            ItemDocument::Directory(cid) => {
                let document: DirectoryDocument = ipfs
                    .get_dag(cid)
                    .deserialized()
                    .await
                    .map_err(anyhow::Error::from)?;

                let directory = document.resolve(ipfs).await?;
                Item::Directory(directory)
            }
            ItemDocument::File(cid) => {
                let document: FileDocument = ipfs
                    .get_dag(cid)
                    .deserialized()
                    .await
                    .map_err(anyhow::Error::from)?;

                let file = document.resolve(ipfs).await?;
                Item::File(file)
            }
        };

        Ok(item)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct FileDocument {
    pub name: String,
    pub size: usize,
    pub thumbnail: Option<Cid>,
    pub favorite: bool,
    pub description: String,
    pub creation: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub file_type: FileType,
    pub reference: Option<Cid>,
    pub hash: Hash,
}

impl FileDocument {
    pub async fn new(ipfs: &Ipfs, file: &File) -> Result<FileDocument, Error> {
        let mut document = FileDocument {
            name: file.name(),
            size: file.size(),
            description: file.description(),
            favorite: file.favorite(),
            creation: file.creation(),
            modified: file.modified(),
            file_type: file.file_type(),
            hash: file.hash(),
            reference: None,
            thumbnail: None,
        };

        if let Some(cid) = file
            .reference()
            .and_then(|refs| refs.parse::<IpfsPath>().ok())
            .and_then(|path| path.root().cid().copied())
        {
            document.reference = ipfs
                .repo()
                .contains(&cid)
                .await
                .unwrap_or_default()
                .then_some(cid)
        }

        if let Some(cid) = file
            .thumbnail_reference()
            .and_then(|refs| refs.parse::<IpfsPath>().ok())
            .and_then(|path| path.root().cid().copied())
        {
            document.thumbnail = ipfs
                .repo()
                .contains(&cid)
                .await
                .unwrap_or_default()
                .then_some(cid)
        }

        Ok(document)
    }

    pub async fn resolve(&self, ipfs: &Ipfs) -> Result<File, Error> {
        let file = File::new(&self.name);
        file.set_description(&self.description);
        file.set_size(self.size);
        file.set_favorite(self.favorite);
        file.set_creation(self.creation);
        file.set_modified(Some(self.modified));
        file.set_hash(self.hash.clone());
        file.set_file_type(self.file_type.clone());

        if let Some(cid) = self.thumbnail {
            let image: ImageDag = ipfs
                .get_dag(cid)
                .timeout(Duration::from_secs(10))
                .deserialized()
                .await?;

            file.set_thumbnail_format(image.mime.into());

            let data = ipfs
                .unixfs()
                .cat(image.link, None, &[], false, Some(Duration::from_secs(10)))
                .await
                .unwrap_or_default();

            file.set_thumbnail(&data);
        }

        if let Some(cid) = self.reference {
            let path = IpfsPath::from(cid);
            file.set_reference(&path.to_string());
        }

        Ok(file)
    }
}

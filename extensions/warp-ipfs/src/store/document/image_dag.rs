use std::future::IntoFuture;

use futures::{future::BoxFuture, stream::BoxStream, FutureExt, StreamExt};
use libipld::Cid;
use rust_ipfs::{Ipfs, PeerId};
use serde::{Deserialize, Serialize};
use warp::{constellation::file::FileType, error::Error, multipass::identity::IdentityImage};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ImageDag {
    pub link: Cid,
    pub size: u64,
    pub mime: FileType,
}

#[tracing::instrument(skip(ipfs, stream))]
pub async fn store_photo(
    ipfs: &Ipfs,
    stream: BoxStream<'static, std::io::Result<Vec<u8>>>,
    file_type: FileType,
    limit: Option<usize>,
) -> Result<Cid, Error> {
    // We dont pin here because we are pinning later
    let mut stream = ipfs.add_unixfs(stream).pin(false);

    let (cid, size) = loop {
        let status = stream.next().await.ok_or(Error::Other)?;

        match status {
            rust_ipfs::unixfs::UnixfsStatus::ProgressStatus { written, .. } => {
                if let Some(limit) = limit {
                    if written > limit {
                        return Err(Error::InvalidLength {
                            context: "photo".into(),
                            current: written,
                            minimum: Some(1),
                            maximum: Some(limit),
                        });
                    }
                }
                tracing::trace!("{written} bytes written");
            }
            rust_ipfs::unixfs::UnixfsStatus::CompletedStatus { path, written, .. } => {
                tracing::debug!("Image is written with {written} bytes - stored at {path}");
                let cid = path.root().cid().copied().ok_or(Error::Other)?;
                break (cid, written);
            }
            rust_ipfs::unixfs::UnixfsStatus::FailedStatus { written, error, .. } => {
                let err = match error {
                    Some(e) => {
                        tracing::error!(
                            "Error uploading picture with {written} bytes written with error: {e}"
                        );
                        e.into()
                    }
                    None => {
                        tracing::error!("Error uploading picture with {written} bytes written");
                        Error::OtherWithContext("Error uploading photo".into())
                    }
                };
                return Err(err);
            }
        }
    };

    let dag = ImageDag {
        link: cid,
        size: size as _,
        mime: file_type,
    };

    let cid = ipfs.dag().put().serialize(dag).pin(true).await?;

    Ok(cid)
}

#[tracing::instrument(skip(ipfs))]
pub fn get_image(ipfs: &Ipfs, cid: Cid) -> GetImage {
    GetImage::new(ipfs, cid)
}

pub(crate) struct GetImage {
    ipfs: Ipfs,
    cid: Cid,
    local: bool,
    peers: Vec<PeerId>,
    limit: Option<usize>,
}

impl GetImage {
    fn new(ipfs: &Ipfs, cid: Cid) -> Self {
        Self {
            ipfs: ipfs.clone(),
            cid,
            local: false,
            peers: vec![],
            limit: None,
        }
    }

    pub fn set_local(mut self, local: bool) -> Self {
        self.local = local;
        self
    }

    pub fn add_peer(mut self, peer: PeerId) -> Self {
        if !self.peers.contains(&peer) {
            self.peers.push(peer)
        }
        self
    }

    pub fn set_limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }
}

impl IntoFuture for GetImage {
    type IntoFuture = BoxFuture<'static, Self::Output>;
    type Output = Result<IdentityImage, Error>;

    fn into_future(self) -> Self::IntoFuture {
        async move {
            let dag: ImageDag = self
                .ipfs
                .get_dag(self.cid)
                .set_local(self.local)
                .deserialized()
                .await?;

            match self.limit {
                Some(size) if dag.size > size as _ => {
                    return Err(Error::InvalidLength {
                        context: "image".into(),
                        current: dag.size as _,
                        minimum: None,
                        maximum: self.limit,
                    });
                }
                Some(_) => {}
                None => {}
            }

            let image = self
                .ipfs
                .cat_unixfs(dag.link)
                .providers(&self.peers)
                .set_local(self.local)
                .await
                .map_err(anyhow::Error::from)?;

            let mut id_img = IdentityImage::default();

            id_img.set_data(image.into());
            id_img.set_image_type(dag.mime);

            Ok(id_img)
        }
        .boxed()
    }
}

use futures::{stream::BoxStream, StreamExt};
use libipld::{serde::to_ipld, Cid};
use rust_ipfs::Ipfs;
use serde::{Deserialize, Serialize};
use std::task::Poll;
use tracing::log;
use warp::{constellation::file::FileType, error::Error, multipass::identity::IdentityImage};

use super::{
    identity::unixfs_fetch,
    utils::{GetDag, GetLocalDag},
};

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
    let mut stream = ipfs.add_unixfs(stream).await?;

    let mut size = 0;

    let cid = futures::future::poll_fn(|cx| loop {
        match stream.poll_next_unpin(cx) {
            Poll::Ready(Some(rust_ipfs::unixfs::UnixfsStatus::ProgressStatus {
                written, ..
            })) => {
                if let Some(limit) = limit {
                    if written > limit {
                        return Poll::Ready(Err(Error::InvalidLength {
                            context: "photo".into(),
                            current: written,
                            minimum: Some(1),
                            maximum: Some(limit),
                        }));
                    }
                }
                log::trace!("{written} bytes written");
            }
            Poll::Ready(Some(rust_ipfs::unixfs::UnixfsStatus::CompletedStatus {
                path,
                written,
                ..
            })) => {
                size = written;
                log::debug!("Image is written with {written} bytes - stored at {path}");
                return Poll::Ready(path.root().cid().copied().ok_or(Error::Other));
            }
            Poll::Ready(Some(rust_ipfs::unixfs::UnixfsStatus::FailedStatus {
                written,
                error,
                ..
            })) => {
                let err = match error {
                    Some(e) => {
                        log::error!(
                            "Error uploading picture with {written} bytes written with error: {e}"
                        );
                        e.into()
                    }
                    None => {
                        log::error!("Error uploading picture with {written} bytes written");
                        Error::OtherWithContext("Error uploading photo".into())
                    }
                };

                return Poll::Ready(Err(err));
            }
            Poll::Ready(None) => return Poll::Ready(Err(Error::ReceiverChannelUnavailable)),
            Poll::Pending => return Poll::Pending,
        }
    })
    .await?;

    let dag = ImageDag {
        link: cid,
        size: size as _,
        mime: file_type,
    };

    let cid = ipfs
        .put_dag(to_ipld(dag).map_err(anyhow::Error::from)?)
        .await?;

    if !ipfs.is_pinned(&cid).await? {
        ipfs.insert_pin(&cid, true).await?;
    }

    Ok(cid)
}

#[tracing::instrument(skip(ipfs))]
pub async fn get_image(
    ipfs: &Ipfs,
    cid: Cid,
    local: bool,
    limit: Option<usize>,
) -> Result<IdentityImage, Error> {
    let dag: ImageDag = match local {
        true => cid.get_local_dag(&ipfs).await?,
        false => cid.get_dag(&ipfs, None).await?,
    };

    let image = unixfs_fetch(ipfs, dag.link, None, local, limit).await?;

    let mut id_img = IdentityImage::default();

    id_img.set_data(image);
    id_img.set_image_type(dag.mime);

    Ok(id_img)
}

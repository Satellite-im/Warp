use futures::{stream::BoxStream, StreamExt};
use libipld::Cid;
use rust_ipfs::{Ipfs, PeerId};
use serde::{Deserialize, Serialize};
use std::task::Poll;
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
    let mut stream = ipfs.add_unixfs(stream);

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
                tracing::trace!("{written} bytes written");
            }
            Poll::Ready(Some(rust_ipfs::unixfs::UnixfsStatus::CompletedStatus {
                path,
                written,
                ..
            })) => {
                size = written;
                tracing::debug!("Image is written with {written} bytes - stored at {path}");
                return Poll::Ready(path.root().cid().copied().ok_or(Error::Other));
            }
            Poll::Ready(Some(rust_ipfs::unixfs::UnixfsStatus::FailedStatus {
                written,
                error,
                ..
            })) => {
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

    let cid = ipfs.dag().put().serialize(dag)?.pin(true).await?;

    Ok(cid)
}

#[tracing::instrument(skip(ipfs))]
pub async fn get_image(
    ipfs: &Ipfs,
    cid: Cid,
    peers: &[PeerId],
    local: bool,
    limit: Option<usize>,
) -> Result<IdentityImage, Error> {
    let mut dag = ipfs.get_dag(cid);

    if local {
        dag = dag.local();
    }

    let dag: ImageDag = dag.deserialized().await?;

    match limit {
        Some(size) if dag.size > size as _ => {
            return Err(Error::InvalidLength {
                context: "image".into(),
                current: dag.size as _,
                minimum: None,
                maximum: limit,
            });
        }
        Some(_) => {}
        None => {}
    }

    let image = ipfs
        .unixfs()
        .cat(dag.link, None, peers, local, None)
        .await
        .map_err(anyhow::Error::from)?;

    let mut id_img = IdentityImage::default();

    id_img.set_data(image);
    id_img.set_image_type(dag.mime);

    Ok(id_img)
}

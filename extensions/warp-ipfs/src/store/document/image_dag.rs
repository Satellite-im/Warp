use futures::StreamExt;
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

#[tracing::instrument(skip(ipfs, opt))]
pub async fn store_photo(
    ipfs: &Ipfs,
    opt: impl Into<rust_ipfs::unixfs::AddOpt>,
    file_type: FileType,
    limit: Option<usize>,
) -> Result<Cid, Error> {
    let mut stream = ipfs.add_unixfs(opt);

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
pub async fn get_image(
    ipfs: &Ipfs,
    cid: Cid,
    peers: &[PeerId],
    local: bool,
    limit: Option<usize>,
) -> Result<IdentityImage, Error> {
    let dag: ImageDag = ipfs.get_dag(cid).set_local(local).deserialized().await?;

    if matches!(limit, Some(size) if dag.size > size as _ ) {
        return Err(Error::InvalidLength {
            context: "image".into(),
            current: dag.size as _,
            minimum: None,
            maximum: limit,
        });
    }

    let size = limit.unwrap_or(dag.size as _);

    let image = ipfs
        .cat_unixfs(dag.link)
        // we apply the limit in the event the stream of bytes exceeds the explicit limit
        // which may be the define sized if `limit` is `Some(_)` or no greater
        // than ImageDag::size, which by usage a limit is usually set as we set a hard limit for
        // profile images and banners to 2MB
        .max_length(size)
        .providers(peers)
        .set_local(local)
        .await
        .map_err(anyhow::Error::from)?;

    let mut id_img = IdentityImage::default();

    id_img.set_data(image.into());
    id_img.set_image_type(dag.mime);

    Ok(id_img)
}

use either::Either;
use futures::StreamExt;
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use warp::{
    crypto::{did_key::CoreSign, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus, Platform, SHORT_ID_SIZE},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdentityDocument {
    pub username: String,

    pub short_id: [u8; SHORT_ID_SIZE],

    pub did: DID,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,

    #[serde(with = "either::serde_untagged")]
    pub profile_picture: Either<Cid, Option<String>>,

    #[serde(with = "either::serde_untagged")]
    pub profile_banner: Either<Cid, Option<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<Vec<u8>>,
}

impl IdentityDocument {
    pub async fn resolve(&self, ipfs: &Ipfs, with_image: bool) -> Result<Identity, Error> {
        self.verify()?;
        let mut identity = Identity::default();
        identity.set_username(&self.username);
        identity.set_did_key(self.did.clone());
        identity.set_short_id(self.short_id);
        identity.set_status_message(self.status_message.clone());
        let mut graphics = identity.graphics();

        if with_image {
            match &self.profile_picture {
                Either::Left(cid) => {
                    match unixfs_fetch(ipfs, *cid, None, true, Some(2 * 1024 * 1024)).await {
                        Ok(data) => {
                            let picture: String = serde_json::from_slice(&data).unwrap_or_default();
                            graphics.set_profile_picture(&picture);
                        }
                        _ => {
                            tokio::spawn({
                                let ipfs = ipfs.clone();
                                let cid = *cid;
                                async move {
                                    unixfs_fetch(&ipfs, cid, None, false, Some(2 * 1024 * 1024))
                                        .await
                                }
                            });
                        }
                    }
                }
                Either::Right(data) => {
                    let data = data.clone().unwrap_or_default();
                    graphics.set_profile_picture(&data);
                }
            }

            match &self.profile_banner {
                Either::Left(cid) => {
                    match unixfs_fetch(ipfs, *cid, None, true, Some(2 * 1024 * 1024)).await {
                        Ok(data) => {
                            let banner: String = serde_json::from_slice(&data).unwrap_or_default();
                            graphics.set_profile_banner(&banner);
                        }
                        _ => {
                            tokio::spawn({
                                let ipfs = ipfs.clone();
                                let cid = *cid;
                                async move {
                                    unixfs_fetch(&ipfs, cid, None, false, Some(2 * 1024 * 1024))
                                        .await
                                }
                            });
                        }
                    }
                }
                Either::Right(data) => {
                    let data = data.clone().unwrap_or_default();
                    graphics.set_profile_banner(&data);
                }
            }
        }

        identity.set_graphics(graphics);

        Ok(identity)
    }

    pub fn sign(mut self, did: &DID) -> Result<Self, Error> {
        let bytes = serde_json::to_vec(&self)?;
        let signature = did.sign(&bytes);
        self.signature = Some(signature);
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), Error> {
        let mut payload = self.clone();

        //TODO: Validate username, short id, and status message

        let signature = std::mem::take(&mut payload.signature).ok_or(Error::InvalidSignature)?;

        let bytes = serde_json::to_vec(&payload)?;
        self.did
            .verify(&bytes, &signature)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }
}

async fn unixfs_fetch(
    ipfs: &Ipfs,
    cid: Cid,
    timeout: Option<Duration>,
    local: bool,
    limit: Option<usize>,
) -> Result<Vec<u8>, Error> {
    let fut = async {
        let stream = ipfs
            .unixfs()
            .cat(IpfsPath::from(cid), None, &[], local)
            .await
            .map_err(anyhow::Error::from)?;

        futures::pin_mut!(stream);

        let mut data = vec![];

        while let Some(stream) = stream.next().await {
            if let Some(limit) = limit {
                if data.len() >= limit {
                    return Err(Error::InvalidLength {
                        context: "data".into(),
                        current: data.len(),
                        minimum: None,
                        maximum: Some(limit),
                    });
                }
            }
            match stream {
                Ok(bytes) => {
                    data.extend(bytes);
                }
                Err(e) => return Err(Error::from(anyhow::anyhow!("{e}"))),
            }
        }
        Ok(data)
    };

    let timeout = timeout.unwrap_or(std::time::Duration::from_secs(15));
    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(data)) => serde_json::from_slice(&data).map_err(Error::from),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
    }
}

use futures::StreamExt;
use libipld::Cid;
use rust_ipfs::{Ipfs, IpfsPath};
use serde::{Deserialize, Serialize};
use std::{hash::Hash, time::Duration};
use warp::{
    crypto::{did_key::CoreSign, DID},
    error::Error,
    multipass::identity::{Identity, IdentityStatus, Platform, SHORT_ID_SIZE},
};

#[derive(Debug, Clone, Deserialize, Serialize, Eq)]
pub struct IdentityDocument {
    pub username: String,

    pub short_id: [u8; SHORT_ID_SIZE],

    pub did: DID,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<Cid>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_banner: Option<Cid>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<IdentityStatus>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl PartialEq for IdentityDocument {
    fn eq(&self, other: &Self) -> bool {
        self.did.eq(&other.did) && self.short_id.eq(&other.short_id)
    }
}

impl Hash for IdentityDocument {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.did.hash(state);
        self.short_id.hash(state);
    }
}

impl IdentityDocument {
    // Used to tell if another identity document is different but also valid
    pub fn different(&self, other: &Self) -> bool {
        if self.ne(other) {
            return false;
        }

        if self.username != other.username
            || self.status_message != other.status_message
            || self.status != other.status
            || self.profile_banner != other.profile_banner
            || self.profile_picture != other.profile_picture
            || self.platform != other.platform
        {
            return other.verify().is_ok();
        }

        false
    }
}

impl IdentityDocument {
    pub fn resolve(&self) -> Result<Identity, Error> {
        self.verify()?;
        let mut identity = Identity::default();
        identity.set_username(&self.username);
        identity.set_did_key(self.did.clone());
        identity.set_short_id(self.short_id);
        identity.set_status_message(self.status_message.clone());

        // let mut fallback = true;

        // if with_image {
        //     if let Some(cid) = self.profile_picture {
        //         match unixfs_fetch(ipfs, cid, None, true, Some(2 * 1024 * 1024)).await {
        //             Ok(data) => {
        //                 let picture: String = serde_json::from_slice(&data).unwrap_or_default();
        //                 if !picture.is_empty() {
        //                     identity.set_profile_picture(&picture);
        //                     fallback = false;
        //                 }
        //             }
        //             Err(_e) => {}
        //         }
        //     }

        //     if fallback {
        //         if let Some(callback) = callback {
        //             let picture = callback(&identity).unwrap_or_default();
        //             // Note: Probably move the callback into a blocking task in case the underlining function make calls that blocks the current thread?
        //             // let picture = tokio::task::spawn_blocking(move || callback(&owned_identity).unwrap_or_default())
        //             //  .await
        //             //  .unwrap_or_default();
        //             if !picture.is_empty() {
        //                 let buffer = String::from_utf8_lossy(&picture).to_string();
        //                 identity.set_profile_picture(&buffer);
        //             }
        //         }
        //     }

        //     if let Some(cid) = self.profile_banner {
        //         match unixfs_fetch(ipfs, cid, None, true, Some(2 * 1024 * 1024)).await {
        //             Ok(data) => {
        //                 let picture: String = serde_json::from_slice(&data).unwrap_or_default();
        //                 identity.set_profile_banner(&picture);
        //             }
        //             Err(_e) => {}
        //         }
        //     }
        // }

        Ok(identity)
    }

    pub fn sign(mut self, did: &DID) -> Result<Self, Error> {
        self.signature = None;
        let bytes = serde_json::to_vec(&self)?;
        let signature = bs58::encode(did.sign(&bytes)).into_string();
        self.signature = Some(signature);
        Ok(self)
    }

    pub fn verify(&self) -> Result<(), Error> {
        let mut payload = self.clone();

        //TODO: Validate username, short id, and status message

        let signature = std::mem::take(&mut payload.signature).ok_or(Error::InvalidSignature)?;
        let signature_bytes = bs58::decode(signature).into_vec()?;
        let bytes = serde_json::to_vec(&payload)?;
        self.did
            .verify(&bytes, &signature_bytes)
            .map_err(|_| Error::InvalidSignature)?;
        Ok(())
    }
}

pub async fn unixfs_fetch(
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

    match local {
        true => fut.await,
        false => {
            let timeout = timeout.unwrap_or(std::time::Duration::from_secs(15));
            match tokio::time::timeout(timeout, fut).await {
                Ok(Ok(data)) => serde_json::from_slice(&data).map_err(Error::from),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(Error::from(anyhow::anyhow!("Timeout at {e}"))),
            }
        }
    }
}

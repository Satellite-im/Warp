
#![allow(dead_code)]
use std::time::Duration;

use ipfs::{Ipfs, Types, IpfsPath};
use futures::{SinkExt, StreamExt, TryFutureExt};
use libipld::{Cid, Ipld};
use warp::{
    error::Error,
    multipass::identity::Identity,
    sync::{Arc, RwLock, Mutex},
    tesseract::Tesseract, crypto::PublicKey,
};

use crate::async_block_unchecked;

pub const IDENTITY_BROADCAST: &'static str = "identity/broadcast";

#[derive(Clone)]
pub struct IdentityStore {
    ipfs: Ipfs<Types>,

    identity: Arc<Mutex<Option<Identity>>>,

    cache: Arc<RwLock<Vec<Identity>>>,
    
    tesseract: Tesseract
}

pub enum LookupBy {
    PublicKey(PublicKey),
    Username(String)
}

impl IdentityStore {
    pub async fn new(ipfs: Ipfs<Types>, tesseract: Tesseract) -> Result<Self, Error> {
        let cache = Arc::new(Default::default());
        let identity = Arc::new(Default::default());

        let store = Self { ipfs, cache, identity, tesseract };

        if let Ok(ident) = store.own_identity().await {
            *store.identity.lock() = Some(ident);
        }
        let id_broadcast_stream = store.ipfs.pubsub_subscribe(IDENTITY_BROADCAST.into()).await?;
        let store_inner = store.clone();

        tokio::spawn(async move {
            let store = store_inner;

            //TODO: Perform "peer discovery" by providing the topic over DHT

            futures::pin_mut!(id_broadcast_stream);
            // Used to send identity out in intervals
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    message = id_broadcast_stream.next() => {
                        if let Some(message) = message {
                            if let Ok(identity) = serde_json::from_slice::<Identity>(&message.data) {
                                if let Ok(own_id) = store.own_identity().await {
                                    if own_id == identity {
                                        continue
                                    }
                                }
                                if store.cache.read().contains(&identity) {
                                    continue;
                                }

                                let index = store.cache.read()
                                    .iter()
                                    .position(|ident| ident.public_key() == identity.public_key());

                                if let Some(index) = index {
                                    store.cache.write().remove(index);
                                }

                                store.cache.write().push(identity);
                                //TODO: Maybe cache the identity in PD
                            }
                        }
                    }
                    _ = tick.tick() => {
                        //TODO: Add check to determine if peers are subscribed to topic before publishing
                        //TODO: Provide a signed payload
                        if let Ok(ident) = store.own_identity().await {
                            if let Ok(bytes) = serde_json::to_vec(&ident) {
                                if let Err(_) = store.ipfs.pubsub_publish(IDENTITY_BROADCAST.into(), bytes).await {
                                    continue
                                }
                            }
                        }
                    }
                }
            }
        });
        Ok(store)
    }

    pub fn lookup(&self, lookup: LookupBy) -> Result<Identity, Error> {
        let identity_list = &*self.cache.read();
        for identity in identity_list.into_iter() {
            match lookup {
                LookupBy::PublicKey(pubkey) if identity.public_key() == pubkey => return Ok(identity.clone()),
                LookupBy::Username(username) if identity.username() == username => return Ok(identity.clone()),
                _ => {}
            }
        }
        Err(Error::IdentityDoesntExist)
    }

    pub async fn own_identity(&self) -> Result<Identity, Error> {
        match self.tesseract.retrieve("ident_cid") {
            Ok(cid) => {
                let cid: Cid = cid.parse().map_err(anyhow::Error::from)?;
                let path = IpfsPath::from(cid);
                let identity = match self.ipfs.get_dag(path).await {
                    Ok(Ipld::Bytes(bytes)) => serde_json::from_slice::<Identity>(&bytes)?,
                    _ => return Err(Error::IdentityDoesntExist), //Note: It should not hit here unless the repo is corrupted
                };
                Ok(identity)
            }
            Err(_) => Err(Error::IdentityDoesntExist),
        }
    }

    pub async fn update_identity(&self) -> Result<(), Error> {
        let ident = self.own_identity().await?;
        *self.identity.lock() = Some(ident);
        Ok(())
    }
}

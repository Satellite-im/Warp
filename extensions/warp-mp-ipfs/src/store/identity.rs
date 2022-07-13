
#![allow(dead_code)]
use std::{time::Duration, sync::atomic::{AtomicBool, Ordering}};

use ipfs::{Ipfs, Types, IpfsPath};
use futures::{SinkExt, StreamExt, TryFutureExt};
use libipld::{Cid, Ipld};
use warp::{
    error::Error,
    multipass::identity::Identity,
    sync::{Arc, RwLock, Mutex},
    tesseract::Tesseract, crypto::PublicKey,
};

use super::IDENTITY_BROADCAST;

#[derive(Clone)]
pub struct IdentityStore {
    ipfs: Ipfs<Types>,

    identity: Arc<RwLock<Option<Identity>>>,

    cache: Arc<RwLock<Vec<Identity>>>,
    
    start_event: Arc<AtomicBool>,

    end_event: Arc<AtomicBool>,

    tesseract: Tesseract
}

impl Drop for IdentityStore {
    fn drop(&mut self) {
        self.disable_event();
        self.end_event();
    }
}

#[derive(Debug, Clone)]
pub enum LookupBy {
    PublicKey(PublicKey),
    Username(String)
}

impl IdentityStore {
    pub async fn new(ipfs: Ipfs<Types>, tesseract: Tesseract) -> Result<Self, Error> {
        let cache = Arc::new(Default::default());
        let identity = Arc::new(Default::default());
        let start_event = Arc::new(Default::default());
        let end_event = Arc::new(Default::default());

        let store = Self { ipfs, cache, identity, start_event, end_event, tesseract };

        if let Ok(ident) = store.own_identity().await {
            *store.identity.write() = Some(ident);
            store.start_event.store(true, Ordering::SeqCst);
        }
        let id_broadcast_stream = store.ipfs.pubsub_subscribe(IDENTITY_BROADCAST.into()).await?;
        let store_inner = store.clone();

        tokio::spawn(async move {
            let store = store_inner;
        

            futures::pin_mut!(id_broadcast_stream);
            // Used to send identity out in intervals
            // TODO: Determine if we can decrease the interval and make it configurable 
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                if store.end_event.load(Ordering::SeqCst) {
                    break
                }
                if !store.start_event.load(Ordering::SeqCst) {
                    continue
                }
                tokio::select! {
                    message = id_broadcast_stream.next() => {
                        if let Some(message) = message {
                            if let Ok(identity) = serde_json::from_slice::<Identity>(&message.data) {
                                if let Some(own_id) = store.identity.read().clone() {
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
                        //TODO: Provide a signed and/or encrypted payload
                        let ident = store.identity.read().clone();
                        if let Some(ident) = ident.as_ref() {
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

    fn cache(&self) -> Vec<Identity> {
        self.cache.read().clone()
    }

    pub fn lookup(&self, lookup: LookupBy) -> Result<Identity, Error> {
        for ident in self.cache() {
            match &lookup {
                LookupBy::PublicKey(pubkey) if &ident.public_key() == pubkey => return Ok(ident.clone()),
                LookupBy::Username(username) if &ident.username() == username => return Ok(ident.clone()),
                _ => continue
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
        *self.identity.write() = Some(ident);
        Ok(())
    }

    pub fn enable_event(&mut self) {
        self.start_event.store(true, Ordering::SeqCst);
    }

    pub fn disable_event(&mut self) {
        self.start_event.store(false, Ordering::SeqCst);
    }

    pub fn end_event(&mut self) {
        self.end_event.store(true, Ordering::SeqCst);
    }
}

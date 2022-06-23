use anyhow::bail;
use libp2p::{core::PublicKey, PeerId};
use std::collections::HashMap;
use warp::error::Error;
use warp::sync::{Arc, Mutex};

/// This registry will account for compatible peers utilizing libp2p through this crate.
#[derive(Debug, Default, Clone)]
pub struct PeerRegistry(Arc<Mutex<Vec<RegisteredPeer>>>);

impl PeerRegistry {
    pub fn list(&self) -> Vec<RegisteredPeer> {
        self.0.lock().clone()
    }

    pub fn exist(&self, option: PeerOption) -> bool {
        let list = self.0.lock();
        for item in list.iter() {
            match option {
                PeerOption::PeerId(id) if item.peer() == id => return true,
                PeerOption::PublicKey(pkey) if item.public_key() == pkey => return true,
                _ => continue,
            }
        }
        false
    }

    pub fn add_public_key(&mut self, public_key: PublicKey) -> bool {
        if self.exist(PeerOption::PublicKey(public_key.clone())) {
            return false;
        }
        let registered_peer = RegisteredPeer::new(public_key);
        self.0.lock().push(registered_peer);
        true
    }

    pub fn add_peer_and_key(&mut self, peer: PeerId, pkey: PublicKey) -> bool {
        if self.exist(PeerOption::PublicKey(pkey.clone())) || self.exist(PeerOption::PeerId(peer)) {
            return false;
        }
        let registered_peer = RegisteredPeer::new_with_peer(peer, pkey);
        self.0.lock().push(registered_peer);
        true
    }

    pub fn remove_peer(&mut self, peer: PeerId) -> bool {
        if self.exist(PeerOption::PeerId(peer)) {
            return false;
        }

        let index = match self
            .0
            .lock()
            .iter()
            .position(|registered_peer| registered_peer.peer() == peer)
        {
            Some(index) => index,
            None => return false,
        };

        self.0.lock().remove(index);

        true
    }
}

pub enum PeerOption {
    PeerId(PeerId),
    PublicKey(PublicKey),
}

#[derive(Debug, Clone)]
pub struct RegisteredPeer {
    peer: PeerId,
    public_key: PublicKey,
}

impl RegisteredPeer {
    pub fn new(public_key: PublicKey) -> RegisteredPeer {
        let peer = PeerId::from(public_key.clone());
        RegisteredPeer { peer, public_key }
    }

    pub fn new_with_peer(peer: PeerId, public_key: PublicKey) -> RegisteredPeer {
        RegisteredPeer { peer, public_key }
    }

    pub fn peer(&self) -> PeerId {
        self.peer
    }

    pub fn public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    pub fn set_peer(&mut self, peer: PeerId) {
        self.peer = peer;
    }

    pub fn set_public_key(&mut self, public_key: PublicKey) {
        self.public_key = public_key;
    }
}

/// Local registry for peers connected to a registered group
#[derive(Debug, Default, Clone)]
pub struct GroupRegistry {
    groups: Arc<Mutex<HashMap<String, Vec<PeerId>>>>,
}

impl GroupRegistry {
    pub fn register_group(&mut self, id: String) -> anyhow::Result<()> {
        if self.groups.lock().contains_key(&id) {
            bail!("Group exist in registry")
        }
        self.groups.lock().insert(id, vec![]);
        Ok(())
    }

    pub fn remove_group(&mut self, id: String) -> anyhow::Result<()> {
        if !self.groups.lock().contains_key(&id) {
            bail!("Group doesnt in registry")
        }
        self.groups.lock().remove(&id);
        Ok(())
    }

    pub fn insert_peer(&mut self, id: String, peer: PeerId) -> anyhow::Result<()> {
        if let Some(group) = self.groups.lock().get_mut(&id) {
            if group.contains(&peer) {
                bail!(Error::IdentityExist)
            }
            group.push(peer);
            return Ok(());
        }
        bail!("Group doesnt exist in registry")
    }

    pub fn remove_peer(&mut self, id: String, peer: PeerId) -> anyhow::Result<()> {
        if let Some(group) = self.groups.lock().get_mut(&id) {
            if !group.contains(&peer) {
                bail!(Error::IdentityDoesntExist)
            }
            let index = group
                .iter()
                .position(|id| *id == peer)
                .ok_or(Error::ArrayPositionNotFound)?;

            group.remove(index);
            return Ok(());
        }
        bail!("Group doesnt exist in registry")
    }

    pub fn exist(&self, id: String) -> bool {
        self.groups.lock().contains_key(&id)
    }

    pub fn exist_in_group(&self, id: String, peer: PeerId) -> bool {
        match self.groups.lock().get(&id) {
            Some(list) => list.contains(&peer),
            None => false,
        }
    }

    pub fn list_groups(&self) -> Vec<String> {
        self.groups.lock().keys().cloned().collect::<Vec<_>>()
    }

    pub fn list_peers(&self, id: String) -> anyhow::Result<Vec<PeerId>> {
        self.groups
            .lock()
            .get(&id)
            .cloned()
            .ok_or(Error::InvalidGroupId)
            .map_err(anyhow::Error::from)
    }
}

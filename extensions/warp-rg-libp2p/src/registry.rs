use libp2p::{core::PublicKey, PeerId};

/// This registry will account for compatible peers utilizing libp2p through this crate.
#[derive(Debug, Default)]
pub struct PeerRegistry(Vec<RegisteredPeer>);

impl AsMut<Vec<RegisteredPeer>> for PeerRegistry {
    fn as_mut(&mut self) -> &mut Vec<RegisteredPeer> {
        &mut self.0
    }
}

impl AsRef<Vec<RegisteredPeer>> for PeerRegistry {
    fn as_ref(&self) -> &Vec<RegisteredPeer> {
        &self.0
    }
}

impl PeerRegistry {
    pub fn list(&self) -> Vec<RegisteredPeer> {
        self.0.clone()
    }

    pub fn exist(&self, option: PeerOption) -> bool {
        for item in self.as_ref() {
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
        self.as_mut().push(registered_peer);
        true
    }

    pub fn add_peer_and_key(&mut self, peer: PeerId, pkey: PublicKey) -> bool {
        if self.exist(PeerOption::PublicKey(pkey.clone())) || self.exist(PeerOption::PeerId(peer)) {
            return false;
        }
        let registered_peer = RegisteredPeer::new_with_peer(peer, pkey);
        self.as_mut().push(registered_peer);
        true
    }

    pub fn remove_peer(&mut self, peer: PeerId) -> bool {
        if self.exist(PeerOption::PeerId(peer)) {
            return false;
        }

        let index = match self
            .as_ref()
            .iter()
            .position(|registered_peer| registered_peer.peer() == peer)
        {
            Some(index) => index,
            None => return false,
        };

        self.as_mut().remove(index);

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

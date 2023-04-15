use std::hash::Hash;

use rust_ipfs::{
    libp2p::swarm::dial_opts::{DialOpts, PeerCondition},
    Ipfs, Multiaddr, PeerId, PublicKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum AgentPriority {
    Primary,
    Secondary,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Default)]
pub enum AgentStatus {
    Online,
    #[default]
    Offline,
}

#[derive(Debug, Clone)]
pub struct Agent {
    name: Option<String>,
    public_key: PublicKey,
    peer_id: PeerId,
    addresses: Vec<Multiaddr>,
    priority: AgentPriority,
    status: AgentStatus,
}

impl From<(PublicKey, AgentPriority)> for Agent {
    fn from((public_key, priority): (PublicKey, AgentPriority)) -> Self {
        Self::new(public_key, priority)
    }
}

impl Hash for Agent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.public_key.hash(state);
    }
}

impl PartialEq for Agent {
    fn eq(&self, other: &Self) -> bool {
        self.public_key.eq(&other.public_key)
    }
}

impl PartialOrd for Agent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.priority.partial_cmp(&other.priority)
    }
}

impl Eq for Agent {}

impl Agent {
    pub fn new(public_key: PublicKey, priority: AgentPriority) -> Self {
        let peer_id = public_key.to_peer_id();
        Self {
            name: None,
            public_key,
            peer_id,
            addresses: vec![],
            priority,
            status: Default::default(),
        }
    }

    pub fn set_name<S: Into<String>>(&mut self, name: S) {
        let name = name.into();
        self.name = Some(name);
    }

    pub fn insert_address(&mut self, address: Multiaddr) {
        if !self.addresses.contains(&address) {
            self.addresses.push(address);
        }
    }

    pub fn remove_address(&mut self, address: &Multiaddr) {
        self.addresses.retain(|addr| addr != address);
    }
}

impl Agent {
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn addresses(&self) -> impl Iterator<Item = &Multiaddr> + '_ {
        self.addresses.iter()
    }

    pub fn priority(&self) -> AgentPriority {
        self.priority
    }

    pub fn status(&self) -> AgentStatus {
        self.status
    }
}

impl Agent {
    pub async fn connect(&mut self, ipfs: &Ipfs) -> Result<(), anyhow::Error> {
        if !ipfs.is_connected(self.peer_id).await? {
            let opt = DialOpts::peer_id(self.peer_id)
                .addresses(self.addresses.clone())
                .condition(PeerCondition::Disconnected)
                .build();
            ipfs.connect(opt).await?;
        }

        if matches!(self.status, AgentStatus::Offline) {
            self.status = AgentStatus::Online;
        }
        Ok(())
    }
}

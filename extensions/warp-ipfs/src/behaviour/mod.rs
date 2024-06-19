pub mod phonebook;

use libp2p::swarm::NetworkBehaviour;
use rust_ipfs::libp2p::{self, swarm::behaviour::toggle::Toggle};

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude", to_swarm = "void::Void")]
pub struct Behaviour {
    pub shuttle_identity: Toggle<crate::shuttle::identity::client::Behaviour>,
    pub shuttle_message: Toggle<crate::shuttle::message::client::Behaviour>,
    pub phonebook: phonebook::Behaviour,
}

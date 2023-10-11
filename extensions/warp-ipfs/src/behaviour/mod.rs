pub mod discovery;
pub mod phonebook;

use libp2p::swarm::NetworkBehaviour;
use rust_ipfs::libp2p;
use rust_ipfs::libp2p::swarm::behaviour::toggle::Toggle;

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude", to_swarm = "void::Void")]
pub struct Behaviour {
    pub shuttle_identity: shuttle::identity::Behaviour,
    pub phonebook: phonebook::Behaviour,
    pub rz_discovery: Toggle<discovery::Behaviour>,
}

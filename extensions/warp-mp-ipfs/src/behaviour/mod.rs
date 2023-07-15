pub mod phonebook;

use libp2p::swarm::NetworkBehaviour;
use rust_ipfs::libp2p;

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude", out_event = "void::Void")]
pub struct Behaviour {
    pub phonebook: phonebook::Behaviour,
}

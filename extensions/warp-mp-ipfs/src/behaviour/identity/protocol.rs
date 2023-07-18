use rust_ipfs::libp2p::request_response::ProtocolName;

use super::PROTOCOL;

#[derive(Clone, Debug)]
pub struct IdentityProtocol;

impl ProtocolName for IdentityProtocol {
    fn protocol_name(&self) -> &[u8] {
        PROTOCOL
    }
}

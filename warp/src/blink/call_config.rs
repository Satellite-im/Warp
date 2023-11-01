use crate::crypto::DID;

#[derive(Default, Debug, Clone)]
pub struct CallConfig {
    pub recording: bool,
    pub self_muted: bool,
    pub self_deafened: bool,
    pub participants_muted: Vec<DID>,
    pub participants_deafened: Vec<DID>,
}

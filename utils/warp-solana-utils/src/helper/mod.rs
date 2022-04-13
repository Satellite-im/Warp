pub mod friends;
pub mod groupchat;
pub mod server;
pub mod user;

//TODO: Have these configurable

use anchor_client::solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

pub fn server_key() -> Pubkey {
    Pubkey::from_str("FGdpP9RSN3ZE8d1PXxiBXS8ThCsXdi342KmDwqSQ3ZBz").unwrap_or_default()
}

pub fn friend_key() -> Pubkey {
    Pubkey::from_str("GjS6t1gK9nktqDJBTjobm9Fdepxg2FGb4vifRDEQ8hXL").unwrap_or_default()
}

pub fn system_program_programid() -> Pubkey {
    Pubkey::from_str("11111111111111111111111111111111").unwrap_or_default()
}

pub mod friends;
pub mod groupchat;
pub mod server;
pub mod user;

use std::str::FromStr;
use warp_common::solana_sdk::pubkey::Pubkey;

pub fn friends_key() -> Pubkey {
    Pubkey::from_str("BxX6o2HG5DWrJt2v8GMSWNG2V2NtxNbAUF3wdE5Ao5gS").unwrap_or_default()
}

pub fn server_key() -> Pubkey {
    Pubkey::from_str("FGdpP9RSN3ZE8d1PXxiBXS8ThCsXdi342KmDwqSQ3ZBz").unwrap_or_default()
}

pub fn groupchat_key() -> Pubkey {
    Pubkey::from_str("bJhvwTYCkQceANgeShZ4xaxUqEBPsV8e1NgRnLRymxs").unwrap_or_default()
}

pub fn user_key() -> Pubkey {
    Pubkey::from_str("7MaC2xrAmmFsuRBEkD6BEL3eJpXCmaikYhLM3eKBPhAH").unwrap_or_default()
}

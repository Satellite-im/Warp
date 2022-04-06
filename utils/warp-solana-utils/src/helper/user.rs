#[allow(unused_imports)]
use super::{friends_key, system_program_programid};
#[allow(unused_imports)]
use crate::pubkey_from_seeds;
#[allow(unused_imports)]
use std::str::FromStr;
#[allow(unused_imports)]
use warp_common::{
    anyhow,
    serde::{Deserialize, Serialize},
    solana_client::rpc_client::RpcClient,
    solana_sdk::{
        account::{Account, ReadableAccount},
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signature},
        signer::Signer,
        transaction::Transaction,
    },
};

pub struct User;

impl User {
    pub fn new() {}
}

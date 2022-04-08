#[allow(unused_imports)]
use super::{friends_key, system_program_programid};
#[allow(unused_imports)]
use crate::pubkey_from_seeds;
#[allow(unused_imports)]
use anchor_client::{
    solana_client::rpc_client::RpcClient,
    solana_sdk::{
        account::{Account, ReadableAccount},
        commitment_config::CommitmentConfig,
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signature},
        signer::Signer,
        system_instruction,
        transaction::Transaction,
    },
    Client, Cluster, Program,
};
use warp_common::anyhow;

#[allow(unused_imports)]
use std::str::FromStr;
use users::User;
#[allow(unused_imports)]
use warp_crypto::rand::rngs::OsRng;

pub struct UserHelper {
    client: Client,
    program: Program,
    payer: Keypair,
}

impl UserHelper {
    pub fn new(payer: Keypair) {}

    pub fn create(&self, name: &str, photo: &str, status: &str) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn get_user(&self, addr: &Pubkey) -> anyhow::Result<User> {
        let user = self.program.account(*addr)?;
        Ok(user)
    }

    pub fn get_current_user(&self) -> anyhow::Result<User> {
        self.get_user(&self.payer.pubkey())
    }
}

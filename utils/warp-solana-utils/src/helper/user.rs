use crate::pubkey_from_seeds;
use anchor_client::{
    solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair},
    Client, Cluster, Program,
};
use warp_common::anyhow;

use anchor_client::solana_sdk::pubkey::Pubkey;
use std::rc::Rc;

use users::User;

pub struct UserHelper {
    pub client: Client,
    pub program: Program,
}

impl UserHelper {
    pub fn new(kp: Keypair) -> Self {
        let client =
            Client::new_with_options(Cluster::Devnet, Rc::new(kp), CommitmentConfig::confirmed());

        let program = client.program(users::id());
        Self { client, program }
    }

    pub fn create(&self, _name: &str, _photo: &str, _status: &str) -> anyhow::Result<()> {
        let user = self.program.payer();

        let (_key, _) = pubkey_from_seeds(&[&user.to_bytes()], "user", &self.program.id())?;

        //TODO: Create user
        Ok(())
    }

    // pub fn get_user(&self, addr: &Pubkey) -> anyhow::Result<User> {
    //     let user = self.program.account(*addr)?;
    //     Ok(user)
    // }
    //
    // pub fn get_current_user(&self) -> anyhow::Result<User> {
    //     self.get_user(&self.program.payer())
    // }
}

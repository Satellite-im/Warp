use crate::manager::SolanaManager;
use crate::pubkey_from_seeds;
use crate::wallet::SolanaWallet;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature};
use anchor_client::solana_sdk::system_program;
use anchor_client::{Client, Cluster, Program};
use std::rc::Rc;
use warp_common::anyhow;
use warp_common::anyhow::anyhow;

#[allow(unused)]
pub struct Friends {
    client: Client,
    program: Program,
    kp: Keypair,
}

#[allow(unused)]
impl Friends {
    pub fn new_with_manager(manager: &SolanaManager) -> anyhow::Result<Self> {
        manager.get_payer_account().map(Self::new_with_keypair)
    }

    pub fn new_with_wallet(wallet: &SolanaWallet) -> anyhow::Result<Self> {
        let kp = wallet.get_keypair()?;
        Ok(Self::new_with_keypair(&kp))
    }

    pub fn new_with_keypair(kp: &Keypair) -> Self {
        //"cheap" way of copying keypair since it does not support copy or clone
        let kp_str = kp.to_base58_string();
        let kp = Keypair::from_base58_string(&kp_str);
        let client = Client::new_with_options(
            Cluster::Devnet,
            Rc::new(Keypair::from_base58_string(&kp_str)),
            CommitmentConfig::confirmed(),
        );

        let program = client.program(friends::id());
        Self {
            client,
            program,
            kp,
        }
    }

    pub fn create_friend_request(&self, friend: &Pubkey) -> anyhow::Result<()> {
        let payer = self.program.payer();
        let (user, key) = pubkey_from_seeds(
            &[&payer.to_bytes(), &friend.to_bytes()],
            "friend",
            &friends::id(),
        )?;
        // let kp = Keypair::new();
        self.program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::MakeRequest {
                request: key,
                user,
                payer,
                system_program: system_program::ID,
            })
            .args(friends::instruction::MakeRequest {
                user1: payer,
                user2: *friend,
                k: "".to_string(),
            })
            .send()?;
        Ok(())
    }

    pub fn get_request(&self, key: Pubkey) -> anyhow::Result<friends::FriendRequest> {
        let key = self.program_key(&key)?;
        let account = self.program.account(key)?;
        Ok(account)
    }

    pub fn accept_friend_request(
        &self,
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Keypair,
        padded: [[u8; 32]; 4],
    ) -> anyhow::Result<Signature> {
        unimplemented!()
    }

    pub fn deny_friend_request(
        &self,
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Keypair,
    ) -> anyhow::Result<Signature> {
        unimplemented!()
    }

    pub fn remove_friend_request(
        &self,
        friend_key: &Pubkey,
        from: &Keypair,
        to: &Pubkey,
    ) -> anyhow::Result<Signature> {
        unimplemented!()
    }

    pub fn remove_friend(&self, friend: (), signer: &Keypair) -> anyhow::Result<Signature> {
        unimplemented!()
    }

    fn program_key(&self, addr: &Pubkey) -> anyhow::Result<Pubkey> {
        let (key, _) = Pubkey::try_find_program_address(
            &[&addr.to_bytes(), &b"friend"[..]],
            &self.program.id(),
        )
        .ok_or_else(|| anyhow!("Error finding program"))?;
        Ok(key)
    }
}

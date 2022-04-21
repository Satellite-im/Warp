use crate::manager::SolanaManager;
#[allow(unused_imports)]
use crate::pubkey_from_seeds;
use crate::wallet::SolanaWallet;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature, Signer};
use anchor_client::solana_sdk::system_program;
use anchor_client::{Client, Cluster, Program};
#[allow(unused_imports)]
use friends::{FriendRequest, Status};
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

    pub fn create_friend_request(
        &self,
        request: &Pubkey,
        friend: &Pubkey,
        key: &str,
    ) -> anyhow::Result<()> {
        let payer = self.program.payer();

        self.program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::MakeRequest {
                request: *request,
                user: payer,
                payer,
                system_program: system_program::ID,
            })
            .args(friends::instruction::MakeRequest {
                user1: payer,
                user2: *friend,
                k: key.to_string(),
            })
            .send()?;
        Ok(())
    }

    pub fn get_request(&self, key: Pubkey) -> anyhow::Result<friends::FriendRequest> {
        let key = self.program_key(&key)?;
        let account = self.program.account(key)?;
        Ok(account)
    }

    pub fn accept_friend_request(&self, request: &Pubkey, key: &str) -> anyhow::Result<Signature> {
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::AcceptRequest {
                request: *request,
                user: self.program.payer(),
            })
            .args(friends::instruction::AcceptRequest { k: key.to_string() })
            .send()?;
        Ok(sig)
    }

    pub fn deny_friend_request(&self, request: &Pubkey) -> anyhow::Result<Signature> {
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::DenyRequest {
                request: *request,
                user: self.program.payer(),
            })
            .send()?;
        Ok(sig)
    }

    pub fn remove_friend_request(&self, request: &Pubkey) -> anyhow::Result<Signature> {
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::RemoveRequest {
                request: *request,
                user: self.program.payer(),
            })
            .send()?;
        Ok(sig)
    }

    pub fn close_friend_request(&self, request: &Pubkey) -> anyhow::Result<Signature> {
        let payer = self.program.payer();
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::CloseRequest {
                request: *request,
                user: payer,
                payer,
            })
            .send()?;
        Ok(sig)
    }

    pub fn remove_friend(&self, request: &Pubkey) -> anyhow::Result<Signature> {
        let payer = self.program.payer();
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::RemoveFriend {
                request: *request,
                user: payer,
            })
            .send()?;
        Ok(sig)
    }

    pub fn compute_account_keys(&self, to: &Pubkey) -> anyhow::Result<(Pubkey, Pubkey)> {
        let (request, _) = Pubkey::try_find_program_address(
            &[&self.kp.pubkey().to_bytes(), &to.to_bytes()],
            &self.program.id(),
        )
        .ok_or_else(|| anyhow!("Error finding program"))?;

        Ok((request, *to))
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

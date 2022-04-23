use crate::helper;
use crate::manager::SolanaManager;
#[allow(unused_imports)]
use crate::pubkey_from_seeds;
use crate::wallet::SolanaWallet;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature};
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

    pub fn create_friend_request(&self, friend: &Pubkey, key: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();
        let (request, from, to) = self.compute_account_keys(*friend)?;

        self.program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::MakeRequest {
                request,
                user: payer,
                payer,
                system_program: system_program::ID,
            })
            .args(friends::instruction::MakeRequest {
                user1: from,
                user2: to,
                k: key.to_string(),
            })
            .send()?;
        Ok(())
    }

    pub fn get_request(&self, key: Pubkey) -> anyhow::Result<friends::FriendRequest> {
        let (request, _, _) = self.compute_account_keys(key)?;
        let account = self.program.account(request)?;
        Ok(account)
    }

    pub fn accept_friend_request(&self, friend: Pubkey, key: &str) -> anyhow::Result<Signature> {
        let (request, _, _) = self.compute_account_keys(friend)?;
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::AcceptRequest {
                request,
                user: self.program.payer(),
            })
            .args(friends::instruction::AcceptRequest { k: key.to_string() })
            .send()?;
        Ok(sig)
    }

    pub fn deny_friend_request(&self, request: Pubkey) -> anyhow::Result<Signature> {
        // let (request, _, _) = self.compute_account_keys(friend)?;
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::DenyRequest {
                request,
                user: self.program.payer(),
            })
            .send()?;
        Ok(sig)
    }

    pub fn remove_friend_request(&self, request: Pubkey) -> anyhow::Result<Signature> {
        // let (request, _, _) = self.compute_account_keys(friend)?;
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::RemoveRequest {
                request,
                user: self.program.payer(),
            })
            .send()?;
        Ok(sig)
    }

    pub fn close_friend_request(&self, request: Pubkey) -> anyhow::Result<Signature> {
        // let (request, _, _) = self.compute_account_keys(request)?;
        let payer = self.program.payer();
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::CloseRequest {
                request,
                user: payer,
                payer,
            })
            .send()?;
        Ok(sig)
    }

    pub fn remove_friend(&self, request: Pubkey) -> anyhow::Result<Signature> {
        let payer = self.program.payer();
        // let (request, _, _) = self.compute_account_keys(friend)?;
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::RemoveFriend {
                request,
                user: payer,
            })
            .send()?;
        Ok(sig)
    }

    pub fn list_requests(&self) -> anyhow::Result<Vec<(Pubkey, FriendRequest)>> {
        let requests = helper::accounts(&self.program, vec![])?;
        Ok(requests)
    }

    fn compute_account_keys(&self, to: Pubkey) -> anyhow::Result<(Pubkey, Pubkey, Pubkey)> {
        let mut list = vec![self.program.payer(), to];
        list.sort_by(|a, b| b.to_bytes().cmp(&a.to_bytes()));

        let (first, second) = (list[0], list[1]);

        let (request, _) = Pubkey::try_find_program_address(
            &[&first.to_bytes(), &second.to_bytes()],
            &self.program.id(),
        )
        .ok_or_else(|| anyhow!("Error finding program"))?;
        Ok((request, first, second))
    }
}

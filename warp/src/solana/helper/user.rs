use anchor_client::{
    solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType},
    solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair},
    Client, Cluster, Program,
};

pub use users::User;

use crate::solana::manager::SolanaManager;
use crate::solana::wallet::SolanaWallet;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anyhow::anyhow;
use std::rc::Rc;

pub struct UserHelper {
    pub client: Client,
    pub program: Program,
    kp: Keypair,
}

impl UserHelper {
    pub fn new_with_manager(manager: &SolanaManager) -> anyhow::Result<Self> {
        Self::new_with_wallet(&manager.wallet)
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

        let program = client.program(users::id());
        Self {
            client,
            program,
            kp,
        }
    }

    pub fn create(&mut self, name: &str, photo_hash: &str, status: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(payer)?;

        self.program
            .request()
            .signer(&self.kp)
            .accounts(users::accounts::Create {
                user,
                signer: payer,
                payer,
                system_program: anchor_client::solana_sdk::system_program::ID,
            })
            .args(users::instruction::Create {
                name: name.to_string(),
                photo_hash: photo_hash.to_string(),
                status: status.to_string(),
            })
            .send()?;
        Ok(())
    }

    pub fn get_user(&self, addr: Pubkey) -> anyhow::Result<User> {
        let key = self.program_key(addr)?;
        self.get_account_raw(key)
    }

    pub fn get_account_raw(&self, addr: Pubkey) -> anyhow::Result<User> {
        let account = self.program.account(addr)?;
        Ok(account)
    }

    //
    pub fn get_current_user(&self) -> anyhow::Result<User> {
        self.get_user(self.program.payer())
    }

    pub fn set_name(&mut self, name: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetName {
                name: name.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    pub fn set_photo(&mut self, hash: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetPhotoHash {
                photo_hash: hash.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    pub fn set_status(&mut self, status: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetStatus {
                status: status.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    pub fn set_banner_image(&mut self, hash: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetBannerImageHash {
                banner_image_hash: hash.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    pub fn set_extra_one(&mut self, data: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetExtraOne {
                extra_1: data.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    pub fn set_extra_two(&mut self, data: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();

        let user = self.program_key(payer)?;
        self.program
            .request()
            .accounts(users::accounts::Modify {
                user,
                signer: payer,
                payer,
            })
            .args(users::instruction::SetExtraTwo {
                extra_2: data.to_string(),
            })
            .signer(&self.kp)
            .send()?;

        Ok(())
    }

    pub fn get_user_by_name(&self, name: &str) -> anyhow::Result<User> {
        let name = bs58::encode(name).into_string();

        let filter = RpcFilterType::Memcmp(Memcmp {
            offset: 12,
            bytes: MemcmpEncodedBytes::Base58(name),
            encoding: None,
        });

        self.program
            .accounts(vec![filter])?
            .iter()
            .cloned()
            .map(|(_, account)| account)
            .collect::<Vec<_>>()
            .first()
            .cloned()
            .ok_or_else(|| anyhow!("User not found"))
    }

    pub fn user_pubkey(&self) -> Pubkey {
        self.program.payer()
    }

    fn program_key(&self, addr: Pubkey) -> anyhow::Result<Pubkey> {
        let (key, _) =
            Pubkey::try_find_program_address(&[&addr.to_bytes(), &b"user"[..]], &self.program.id())
                .ok_or_else(|| anyhow!("Error finding program"))?;
        Ok(key)
    }
}

use crate::manager::SolanaManager;
use crate::wallet::SolanaWallet;
use crate::Pubkey;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::{Client, Cluster, Program};
use groupchats::{Group, Invitation};
use std::rc::Rc;
use warp_common::anyhow;
use warp_common::anyhow::anyhow;

#[allow(unused)]
pub struct GroupChat {
    client: Client,
    program: Program,
    kp: Keypair,
}

#[allow(unused)]
impl GroupChat {
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

    pub fn create_group(&self, id: &str, name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn invite(&self, id: &str, recipient: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn group_pubkey(&self, hash: &[u8]) -> anyhow::Result<(Pubkey, u8)> {
        Pubkey::try_find_program_address(&[hash, &b"groupchat"[..]], &self.program.id())
            .ok_or_else(|| anyhow!("Error finding program"))
    }

    fn invite_pubkey(&self, user: Pubkey, group: Pubkey) -> anyhow::Result<Pubkey> {
        let (key, _) = Pubkey::try_find_program_address(
            &[&user.to_bytes(), &group.to_bytes(), &b"groupchat"[..]],
            &self.program.id(),
        )
        .ok_or_else(|| anyhow!("Error finding program"))?;
        Ok(key)
    }

    fn group_address_from_id(&self, id: &str) -> anyhow::Result<Pubkey> {
        let hash = warp_crypto::hash::sha256_hash(id.as_bytes(), None)?; //map(warp_common::hex::encode)
        let (key, _) = self.group_pubkey(&hash)?;
        Ok(key)
    }

    fn get_invitation_accounts(&self) -> anyhow::Result<Invitation> {
        unimplemented!()
    }

    fn get_invite_by_group_id(&self, id: &str) -> anyhow::Result<Invitation> {
        unimplemented!()
    }

    pub fn get_user_groups(&self, addr: Pubkey) -> anyhow::Result<Vec<Group>> {
        unimplemented!()
    }
}

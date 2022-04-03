use super::friends_key;
use crate::manager::SolanaManager;
use crate::pubkey_from_seeds;
use crate::wallet::SolanaWallet;
use std::str::FromStr;
use warp_common::anyhow;
use warp_common::solana_client::rpc_client::RpcClient;
use warp_common::solana_sdk::instruction::{AccountMeta, Instruction};
use warp_common::solana_sdk::message::Message;
use warp_common::solana_sdk::pubkey::Pubkey;
use warp_common::solana_sdk::signature::Keypair;
use warp_common::solana_sdk::signer::Signer;
use warp_common::solana_sdk::transaction::Transaction;

pub struct Friends;

pub enum FriendParam {
    Create { friend: Pubkey },
}

impl Friends {
    pub fn create_derived_account(
        client: &RpcClient,
        payer: &Keypair,
        seed: Pubkey,
        seed_str: &str,
        params: FriendParam,
        opt: Option<()>,
    ) -> anyhow::Result<()> {
        let FriendParam::Create { friend } = params;
        let (base, key) = pubkey_from_seeds(
            &[&seed.to_bytes(), &friend.to_bytes()],
            seed_str,
            &friends_key(),
        )?;
        let rent_key = Pubkey::from_str("SysvarRent111111111111111111111111111111111")?;
        let instruction = Instruction::new_with_bincode(
            friends_key(),
            &(),
            vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new_readonly(seed, false),
                AccountMeta::new_readonly(base, false),
                AccountMeta::new(key, false),
                AccountMeta::new(rent_key, false), //SystemProgram?,
            ],
        );

        let message = Message::new(&[instruction], None); //Set payer?
        let mut transaction = Transaction::new(&[payer], message, client.get_latest_blockhash()?);
        let s = client.send_and_confirm_transaction(&transaction)?;
        Ok(())
    }

    pub fn create_friend(
        client: &RpcClient,
        from: Pubkey,
        to: Pubkey,
        opt: Option<()>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

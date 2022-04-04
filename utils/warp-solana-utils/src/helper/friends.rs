use super::{friends_key, system_program_programid};
use crate::pubkey_from_seeds;
use std::str::FromStr;
use warp_common::anyhow;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::solana_client::rpc_client::RpcClient;
use warp_common::solana_sdk::account::{Account, ReadableAccount};
use warp_common::solana_sdk::instruction::{AccountMeta, Instruction};
use warp_common::solana_sdk::message::Message;
use warp_common::solana_sdk::pubkey::Pubkey;
use warp_common::solana_sdk::signature::{Keypair, Signature};
use warp_common::solana_sdk::signer::Signer;
use warp_common::solana_sdk::transaction::Transaction;

//TODO: Move connection and payer to the struct as fields
pub struct Friends;

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "warp_common::serde", rename_all = "camelCase")]
pub enum FriendParam {
    CreateAccount { friend: FriendKey },
    MakeRequest { tex: [[u8; 32]; 4] },
    AcceptRequest { tex: [[u8; 32]; 4] },
    DenyRequest,
    RemoveRequest,
    RemoveFriend,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "warp_common::serde", rename_all = "snake_case")]
pub enum FriendStatus {
    NotAssigned,
    Pending,
    Accepted,
    Refused,
    Removed,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "warp_common::serde", rename_all = "snake_case")]
pub enum FriendEvents {
    NewRequest,
    NewFriend,
    RequestDenied,
    RequestRemoved,
    FriendRemoved,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "warp_common::serde", rename_all = "camelCase")]
pub struct FriendKey {
    pub friendkey: [u8; 32],
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "warp_common::serde")]
pub struct FriendAccount {
    id: String,
    from: String,
    status: i64,
    from_mailbox_id: String,
    to_mailbox_id: String,
    to: String,
}

impl Friends {
    pub fn create_derived_account(
        client: &RpcClient,
        payer: &Keypair,
        seed: &Pubkey,
        seed_str: &str,
        params: FriendParam,
    ) -> anyhow::Result<Pubkey> {
        let friend = if let FriendParam::CreateAccount { friend } = &params {
            friend
        } else {
            anyhow::bail!("params requires Friend::CreateAccount")
        };

        let (base, key) = pubkey_from_seeds(
            &[&seed.to_bytes(), &friend.friendkey],
            seed_str,
            &friends_key(),
        )?;
        let rent_key = Pubkey::from_str("SysvarRent111111111111111111111111111111111")?;
        let instruction = Instruction::new_with_bincode(
            friends_key(),
            &params,
            vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new_readonly(*seed, false),
                AccountMeta::new_readonly(base, false),
                AccountMeta::new(key, false),
                AccountMeta::new(rent_key, false), //SystemProgram?,
                AccountMeta::new_readonly(system_program_programid(), false),
            ],
        );

        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let transaction = Transaction::new(&[payer], message, client.get_latest_blockhash()?);
        client.send_and_confirm_transaction(&transaction)?;
        Ok(key)
    }

    pub fn create_friend(
        client: &RpcClient,
        payer: &Keypair,
        from: &Pubkey,
        to: &Pubkey,
    ) -> anyhow::Result<Pubkey> {
        Friends::create_derived_account(
            client,
            payer,
            from,
            "friends",
            FriendParam::CreateAccount {
                friend: FriendKey {
                    friendkey: to.to_bytes(),
                },
            },
        )
    }

    pub fn create_friend_request(
        client: &RpcClient,
        payer: &Keypair,
        friend_key: &Pubkey,
        friend_key2: &Pubkey,
        from: &Keypair,
        to: &Pubkey,
        padded: [[u8; 32]; 4],
    ) -> anyhow::Result<Signature> {
        let instruction = Self::make_friend_request_instruction(
            friend_key,
            friend_key2,
            &from.pubkey(),
            to,
            padded,
        )?;
        let message = Message::new(&[instruction], Some(&payer.pubkey()));

        let transaction = Transaction::new(&[payer], message, client.get_latest_blockhash()?);
        let signature = client.send_and_confirm_transaction(&transaction)?;

        Ok(signature)
    }

    pub fn accept_friend_request(
        client: &RpcClient,
        payer: &Keypair,
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Keypair,
        padded: [[u8; 32]; 4],
    ) -> anyhow::Result<Signature> {
        let instruction =
            Self::accept_friend_request_instruction(friend_key, from, &to.pubkey(), padded)?;
        let message = Message::new(&[instruction], Some(&payer.pubkey()));

        let transaction = Transaction::new(&[payer, to], message, client.get_latest_blockhash()?);
        let signature = client.send_and_confirm_transaction(&transaction)?;

        Ok(signature)
    }

    pub fn deny_friend_request(
        client: &RpcClient,
        payer: &Keypair,
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Keypair,
    ) -> anyhow::Result<Signature> {
        let instruction = Self::deny_friend_request_instruction(friend_key, from, &to.pubkey())?;
        let message = Message::new(&[instruction], Some(&payer.pubkey()));

        let transaction = Transaction::new(&[payer, to], message, client.get_latest_blockhash()?);
        let signature = client.send_and_confirm_transaction(&transaction)?;

        Ok(signature)
    }

    pub fn remove_friend_request(
        client: &RpcClient,
        payer: &Keypair,
        friend_key: &Pubkey,
        from: &Keypair,
        to: &Pubkey,
    ) -> anyhow::Result<Signature> {
        let instruction = Self::remove_friend_request_instruction(friend_key, &from.pubkey(), to)?;
        let message = Message::new(&[instruction], Some(&payer.pubkey()));

        let transaction = Transaction::new(&[payer, from], message, client.get_latest_blockhash()?);
        let signature = client.send_and_confirm_transaction(&transaction)?;

        Ok(signature)
    }

    pub fn remove_friend(
        client: &RpcClient,
        payer: &Keypair,
        friend: FriendAccount,
        signer: &Keypair,
    ) -> anyhow::Result<Signature> {
        let friend_key = Pubkey::from_str(&friend.id)?;
        let from_key = Pubkey::from_str(&friend.from)?;
        let to_key = Pubkey::from_str(&friend.to)?;

        let initiator = from_key == signer.pubkey();

        let instruction =
            Self::remove_friend_instruction(&friend_key, &from_key, &to_key, initiator)?;
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let transaction =
            Transaction::new(&[payer, signer], message, client.get_latest_blockhash()?);
        let signature = client.send_and_confirm_transaction(&transaction)?;
        Ok(signature)
    }

    //TODO: Convert the data into a proper format
    pub fn get_friend(client: &RpcClient, friend_key: &Pubkey) -> anyhow::Result<Account> {
        client
            .get_account(friend_key)
            .map_err(|e| anyhow::anyhow!("Get friend error: {:?}", e))
    }

    pub fn get_parsed_friend(
        client: &RpcClient,
        friend_key: &Pubkey,
    ) -> anyhow::Result<FriendAccount> {
        let friend = Self::get_friend(client, friend_key)?;
        let account = bincode::deserialize(friend.data())
            .map_err(|e| anyhow::anyhow!("Error here: {:?}", e))?;
        Ok(account)
    }

    pub fn make_friend_request_instruction(
        friend_key: &Pubkey,
        friend_key2: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
        padded: [[u8; 32]; 4],
    ) -> anyhow::Result<Instruction> {
        let rent = Pubkey::from_str("SysvarRent111111111111111111111111111111111")?;
        Ok(Instruction::new_with_bincode(
            friends_key(),
            &FriendParam::MakeRequest { tex: padded },
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new(*friend_key2, false),
                AccountMeta::new_readonly(*from, true),
                AccountMeta::new(*to, false),
                AccountMeta::new_readonly(rent, false),
            ],
        ))
    }

    pub fn accept_friend_request_instruction(
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
        padded: [[u8; 32]; 4],
    ) -> anyhow::Result<Instruction> {
        let rent = Pubkey::from_str("SysvarRent111111111111111111111111111111111")?;
        Ok(Instruction::new_with_bincode(
            friends_key(),
            &FriendParam::AcceptRequest { tex: padded },
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new(*from, false),
                AccountMeta::new_readonly(*to, true),
                AccountMeta::new_readonly(rent, false),
            ],
        ))
    }

    pub fn deny_friend_request_instruction(
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
    ) -> anyhow::Result<Instruction> {
        let rent = Pubkey::from_str("SysvarRent111111111111111111111111111111111")?;
        Ok(Instruction::new_with_bincode(
            friends_key(),
            &FriendParam::DenyRequest,
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new(*from, false),
                AccountMeta::new_readonly(*to, true),
                AccountMeta::new_readonly(rent, false),
            ],
        ))
    }

    pub fn remove_friend_request_instruction(
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
    ) -> anyhow::Result<Instruction> {
        let rent = Pubkey::from_str("SysvarRent111111111111111111111111111111111")?;
        Ok(Instruction::new_with_bincode(
            friends_key(),
            &FriendParam::RemoveRequest,
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new_readonly(*from, true),
                AccountMeta::new(*to, false),
                AccountMeta::new_readonly(rent, false),
            ],
        ))
    }

    pub fn remove_friend_instruction(
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
        initiator: bool,
    ) -> anyhow::Result<Instruction> {
        let rent = Pubkey::from_str("SysvarRent111111111111111111111111111111111")?;
        Ok(Instruction::new_with_bincode(
            friends_key(),
            &FriendParam::RemoveFriend,
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new_readonly(*from, initiator),
                AccountMeta::new_readonly(*to, !initiator),
                AccountMeta::new_readonly(rent, false),
            ],
        ))
    }
}

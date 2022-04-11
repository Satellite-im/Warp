use crate::{pubkey_from_seeds, EndPoint};
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_client::rpc_config::RpcProgramAccountsConfig;
use anchor_client::solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use anchor_client::solana_sdk::account::{Account, ReadableAccount};
use anchor_client::solana_sdk::instruction::{AccountMeta, Instruction};
use anchor_client::solana_sdk::message::Message;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature};
use anchor_client::solana_sdk::signer::Signer;
use anchor_client::solana_sdk::system_program;
use anchor_client::solana_sdk::transaction::Transaction;
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use bs58::encode::EncodeBuilder;
use std::str::FromStr;
use warp_common::anyhow;
use warp_common::serde::{Deserialize, Serialize};

//TODO: Move connection and payer to the struct as fields
pub struct Friends {
    connection: RpcClient,
    payer: Keypair,
}

#[derive(Serialize, Deserialize, BorshSerialize, BorshDeserialize, BorshSchema, Debug)]
#[serde(crate = "warp_common::serde", rename_all = "camelCase")]
pub enum FriendParam {
    CreateAccount { friend: FriendKey },
    MakeRequest { tex: [[u8; 32]; 4] },
    AcceptRequest { tex: [[u8; 32]; 4] },
    DenyRequest,
    RemoveRequest,
    RemoveFriend,
}

#[derive(Serialize, Deserialize, BorshSerialize, BorshDeserialize, BorshSchema, Debug, Clone)]
#[serde(crate = "warp_common::serde", rename_all = "snake_case")]
pub enum FriendStatus {
    NotAssigned,
    Pending,
    Accepted,
    Refused,
    Removed,
}

#[derive(Serialize, Deserialize, BorshSerialize, BorshDeserialize, BorshSchema, Debug)]
#[serde(crate = "warp_common::serde", rename_all = "snake_case")]
pub enum FriendEvents {
    NewRequest,
    NewFriend,
    RequestDenied,
    RequestRemoved,
    FriendRemoved,
}

#[derive(Serialize, Deserialize, BorshSerialize, BorshDeserialize, BorshSchema, Debug)]
#[serde(crate = "warp_common::serde", rename_all = "camelCase")]
pub struct FriendKey {
    pub friendkey: [u8; 32],
}

#[derive(BorshSerialize, BorshDeserialize, BorshSchema, PartialEq, Debug)]
pub struct FriendAccount {
    id: String,
    from: String,
    status: i64,
    from_mailbox_id: String,
    to_mailbox_id: String,
    to: String,
}

impl Friends {
    pub fn new(connection: EndPoint, payer: Keypair) -> Self {
        Self {
            payer,
            connection: RpcClient::new(connection.to_string()),
        }
    }
    pub fn create_derived_account(
        &self,
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
            &super::friend_key(),
        )?;
        //TODO: Determine if we should rely on bincode or borsh
        let instruction = Instruction::new_with_bincode(
            super::friend_key(),
            &params,
            vec![
                AccountMeta::new(self.payer.pubkey(), true),
                AccountMeta::new_readonly(*seed, false),
                AccountMeta::new_readonly(base, false),
                AccountMeta::new(key, false),
                AccountMeta::new(anchor_client::solana_sdk::sysvar::rent::id(), false), //SystemProgram?,
                AccountMeta::new_readonly(system_program::ID, false),
            ],
        );

        let message = Message::new(&[instruction], Some(&self.payer.pubkey()));
        let transaction = Transaction::new(
            &[&self.payer],
            message,
            self.connection.get_latest_blockhash()?,
        );
        self.connection.send_and_confirm_transaction(&transaction)?;
        Ok(key)
    }

    pub fn create_friend(&self, from: &Pubkey, to: &Pubkey) -> anyhow::Result<Pubkey> {
        self.create_derived_account(
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
        &self,
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
        let message = Message::new(&[instruction], Some(&self.payer.pubkey()));

        let transaction = Transaction::new(
            &[&self.payer],
            message,
            self.connection.get_latest_blockhash()?,
        );
        let signature = self.connection.send_and_confirm_transaction(&transaction)?;

        Ok(signature)
    }

    pub fn accept_friend_request(
        &self,
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Keypair,
        padded: [[u8; 32]; 4],
    ) -> anyhow::Result<Signature> {
        let instruction =
            Self::accept_friend_request_instruction(friend_key, from, &to.pubkey(), padded)?;
        let message = Message::new(&[instruction], Some(&self.payer.pubkey()));

        let transaction = Transaction::new(
            &[&self.payer, to],
            message,
            self.connection.get_latest_blockhash()?,
        );
        let signature = self.connection.send_and_confirm_transaction(&transaction)?;

        Ok(signature)
    }

    pub fn deny_friend_request(
        &self,
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Keypair,
    ) -> anyhow::Result<Signature> {
        let instruction = Self::deny_friend_request_instruction(friend_key, from, &to.pubkey())?;
        let message = Message::new(&[instruction], Some(&self.payer.pubkey()));

        let transaction = Transaction::new(
            &[&self.payer, to],
            message,
            self.connection.get_latest_blockhash()?,
        );
        let signature = self.connection.send_and_confirm_transaction(&transaction)?;

        Ok(signature)
    }

    pub fn remove_friend_request(
        &self,
        friend_key: &Pubkey,
        from: &Keypair,
        to: &Pubkey,
    ) -> anyhow::Result<Signature> {
        let instruction = Self::remove_friend_request_instruction(friend_key, &from.pubkey(), to)?;
        let message = Message::new(&[instruction], Some(&self.payer.pubkey()));

        let transaction = Transaction::new(
            &[&self.payer, from],
            message,
            self.connection.get_latest_blockhash()?,
        );
        let signature = self.connection.send_and_confirm_transaction(&transaction)?;

        Ok(signature)
    }

    pub fn remove_friend(
        &self,
        friend: FriendAccount,
        signer: &Keypair,
    ) -> anyhow::Result<Signature> {
        let friend_key = Pubkey::from_str(&friend.id)?;
        let from_key = Pubkey::from_str(&friend.from)?;
        let to_key = Pubkey::from_str(&friend.to)?;

        let initiator = from_key == signer.pubkey();

        let instruction =
            Self::remove_friend_instruction(&friend_key, &from_key, &to_key, initiator)?;
        let message = Message::new(&[instruction], Some(&self.payer.pubkey()));
        let transaction = Transaction::new(
            &[&self.payer, signer],
            message,
            self.connection.get_latest_blockhash()?,
        ); //get_latest_blockhash
        let signature = self.connection.send_and_confirm_transaction(&transaction)?;
        Ok(signature)
    }

    //TODO: Convert the data into a proper format
    pub fn get_friend(&self, friend_key: &Pubkey) -> anyhow::Result<Account> {
        self.connection
            .get_account(friend_key)
            .map_err(|e| anyhow::anyhow!("Get friend error: {:?}", e))
    }

    pub fn get_parsed_friend(&self, friend_key: &Pubkey) -> anyhow::Result<FriendAccount> {
        let friend = self.get_friend(friend_key)?;
        let account = borsh::try_from_slice_with_schema(&friend.data())?;
        Ok(account)
    }

    pub fn compute_friend_account_key(&self, from: Pubkey, to: Pubkey) -> anyhow::Result<Pubkey> {
        let (_, key) = pubkey_from_seeds(
            &[&from.to_bytes(), &to.to_bytes()],
            "friend",
            &super::friend_key(),
        )?;

        Ok(key)
    }

    pub fn get_friend_account_by_status(
        &self,
        status: FriendStatus,
    ) -> anyhow::Result<(Vec<(Pubkey, Account)>, Vec<(Pubkey, Account)>)> {
        let from_friend_status = borsh::to_vec(&(self.payer.pubkey().to_bytes(), status.clone()))
            .map(bs58::encode)
            .map(EncodeBuilder::into_vec)?;

        let to_friend_status = borsh::to_vec(&(status.clone(), self.payer.pubkey().to_bytes()))
            .map(bs58::encode)
            .map(EncodeBuilder::into_vec)?;

        let mut outgoing_filter = RpcProgramAccountsConfig::default();
        outgoing_filter.filters = Some(vec![RpcFilterType::Memcmp(Memcmp {
            offset: 0,
            bytes: MemcmpEncodedBytes::Bytes(from_friend_status),
            encoding: None,
        })]);

        let mut incoming_filter = RpcProgramAccountsConfig::default();
        incoming_filter.filters = Some(vec![RpcFilterType::Memcmp(Memcmp {
            offset: 32,
            bytes: MemcmpEncodedBytes::Bytes(to_friend_status),
            encoding: None,
        })]);

        let outgoing = self
            .connection
            .get_program_accounts_with_config(&super::friend_key(), outgoing_filter)?
            .to_vec();

        let incoming = self
            .connection
            .get_program_accounts_with_config(&super::friend_key(), incoming_filter)?
            .to_vec();

        Ok((incoming, outgoing))
    }

    pub fn make_friend_request_instruction(
        friend_key: &Pubkey,
        friend_key2: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
        padded: [[u8; 32]; 4],
    ) -> anyhow::Result<Instruction> {
        Ok(Instruction::new_with_bincode(
            super::friend_key(),
            &FriendParam::MakeRequest { tex: padded },
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new(*friend_key2, false),
                AccountMeta::new_readonly(*from, true),
                AccountMeta::new(*to, false),
                AccountMeta::new_readonly(anchor_client::solana_sdk::sysvar::rent::id(), false),
            ],
        ))
    }

    pub fn accept_friend_request_instruction(
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
        padded: [[u8; 32]; 4],
    ) -> anyhow::Result<Instruction> {
        Ok(Instruction::new_with_bincode(
            super::friend_key(),
            &FriendParam::AcceptRequest { tex: padded },
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new(*from, false),
                AccountMeta::new_readonly(*to, true),
                AccountMeta::new_readonly(anchor_client::solana_sdk::sysvar::rent::id(), false),
            ],
        ))
    }

    pub fn deny_friend_request_instruction(
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
    ) -> anyhow::Result<Instruction> {
        Ok(Instruction::new_with_bincode(
            super::friend_key(),
            &FriendParam::DenyRequest,
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new(*from, false),
                AccountMeta::new_readonly(*to, true),
                AccountMeta::new_readonly(anchor_client::solana_sdk::sysvar::rent::id(), false),
            ],
        ))
    }

    pub fn remove_friend_request_instruction(
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
    ) -> anyhow::Result<Instruction> {
        Ok(Instruction::new_with_bincode(
            super::friend_key(),
            &FriendParam::RemoveRequest,
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new_readonly(*from, true),
                AccountMeta::new(*to, false),
                AccountMeta::new_readonly(anchor_client::solana_sdk::sysvar::rent::id(), false),
            ],
        ))
    }

    pub fn remove_friend_instruction(
        friend_key: &Pubkey,
        from: &Pubkey,
        to: &Pubkey,
        initiator: bool,
    ) -> anyhow::Result<Instruction> {
        Ok(Instruction::new_with_bincode(
            super::friend_key(),
            &FriendParam::RemoveFriend,
            vec![
                AccountMeta::new(*friend_key, false),
                AccountMeta::new_readonly(*from, initiator),
                AccountMeta::new_readonly(*to, !initiator),
                AccountMeta::new_readonly(anchor_client::solana_sdk::sysvar::rent::id(), false),
            ],
        ))
    }
}

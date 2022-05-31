use crate::solana::error::FriendsError;
use crate::solana::manager::SolanaManager;
use crate::solana::wallet::SolanaWallet;
use anchor_client::anchor_lang::prelude::ProgramError;
use anchor_client::solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signature};
use anchor_client::solana_sdk::system_program;
use anchor_client::{Client, ClientError, Cluster, Program};
use anyhow::anyhow;
use friends::{FriendRequest, Status};
#[allow(unused_imports)]
use std::rc::Rc;

pub type RequestList = Vec<(Pubkey, FriendRequest)>;

#[allow(unused)]
pub struct Friends {
    client: Client,
    program: Program,
    kp: Keypair,
}

#[allow(unused)]
impl Friends {
    pub fn new_with_manager(manager: &SolanaManager) -> anyhow::Result<Self> {
        Ok(Self::new_with_cluster(
            manager.cluster.clone(),
            &manager.wallet.get_keypair()?,
        ))
    }

    pub fn devnet_with_wallet(wallet: &SolanaWallet) -> anyhow::Result<Self> {
        let kp = wallet.get_keypair()?;
        Ok(Self::new_with_cluster(Cluster::Devnet, &kp))
    }

    pub fn devnet_keypair(kp: &Keypair) -> Self {
        Self::new_with_cluster(Cluster::Devnet, kp)
    }

    pub fn mainnet_with_wallet(wallet: &SolanaWallet) -> anyhow::Result<Self> {
        let kp = wallet.get_keypair()?;
        Ok(Self::new_with_cluster(Cluster::Mainnet, &kp))
    }

    pub fn mainnet_keypair(kp: &Keypair) -> Self {
        Self::new_with_cluster(Cluster::Mainnet, kp)
    }

    pub fn new_with_cluster(cluster: Cluster, kp: &Keypair) -> Self {
        let kp_str = kp.to_base58_string();
        let kp = Keypair::from_base58_string(&kp_str);
        let client = Client::new_with_options(
            cluster,
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

    pub fn create_friend_request(&self, friend: Pubkey, key: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();
        let (request, from, to) = self.compute_account_keys(friend)?;

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
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    pub fn get_request(&self, key: Pubkey) -> anyhow::Result<friends::FriendRequest> {
        let (request, _, _) = self.compute_account_keys(key)?;
        let account = self.program.account(request).map_err(|e| match e {
            ClientError::ProgramError(ProgramError::Custom(code)) => {
                anyhow!(FriendsError::from(code))
            }
            _ => anyhow!(e),
        })?;
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
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(sig)
    }

    pub fn deny_friend_request(&self, request: Pubkey) -> anyhow::Result<Signature> {
        let (request, _, _) = self.compute_account_keys(request)?;
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::DenyRequest {
                request,
                user: self.program.payer(),
            })
            .args(friends::instruction::DenyRequest)
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(sig)
    }

    pub fn remove_friend_request(&self, request: Pubkey) -> anyhow::Result<Signature> {
        let (request, _, _) = self.compute_account_keys(request)?;
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::RemoveRequest {
                request,
                user: self.program.payer(),
            })
            .args(friends::instruction::RemoveRequest)
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(sig)
    }

    pub fn close_friend_request(&self, request: Pubkey) -> anyhow::Result<Signature> {
        let (request, _, _) = self.compute_account_keys(request)?;
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
            .args(friends::instruction::CloseRequest)
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(sig)
    }

    pub fn remove_friend(&self, request: Pubkey) -> anyhow::Result<Signature> {
        let payer = self.program.payer();
        let (request, _, _) = self.compute_account_keys(request)?;
        let sig = self
            .program
            .request()
            .signer(&self.kp)
            .accounts(friends::accounts::RemoveFriend {
                request,
                user: payer,
            })
            .args(friends::instruction::RemoveFriend)
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(sig)
    }

    pub fn list_outgoing_request(&self) -> anyhow::Result<RequestList> {
        let outgoing_filter = vec![RpcFilterType::Memcmp(Memcmp {
            offset: 8,
            bytes: MemcmpEncodedBytes::Base58(self.program.payer().to_string()),
            encoding: None,
        })];

        let outgoing = self
            .program
            .accounts(outgoing_filter)
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(outgoing)
    }

    pub fn list_incoming_request(&self) -> anyhow::Result<RequestList> {
        let incoming_filter = vec![RpcFilterType::Memcmp(Memcmp {
            offset: 41,
            bytes: MemcmpEncodedBytes::Base58(self.program.payer().to_string()),
            encoding: None,
        })];

        let incoming = self
            .program
            .accounts(incoming_filter)
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(incoming)
    }

    pub fn list_all(&self) -> anyhow::Result<(RequestList, RequestList)> {
        let payer = self.program.payer();

        let outgoing_filter = vec![RpcFilterType::Memcmp(Memcmp {
            offset: 8,
            bytes: MemcmpEncodedBytes::Base58(payer.to_string()),
            encoding: None,
        })];

        let incoming_filter = vec![RpcFilterType::Memcmp(Memcmp {
            offset: 41,
            bytes: MemcmpEncodedBytes::Base58(payer.to_string()),
            encoding: None,
        })];

        let outgoing = self
            .program
            .accounts(outgoing_filter)
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        let incoming = self
            .program
            .accounts(incoming_filter)
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;

        Ok((outgoing, incoming))
    }

    pub fn list_all_by_status(
        &self,
        status: DirectStatus,
    ) -> anyhow::Result<(RequestList, RequestList)> {
        let payer = self.program.payer();
        let status = borsh::to_vec(&Status::from(status))?;

        let outgoing_filter = vec![
            RpcFilterType::Memcmp(Memcmp {
                offset: 8,
                bytes: MemcmpEncodedBytes::Base58(payer.to_string()),
                encoding: None,
            }),
            RpcFilterType::Memcmp(Memcmp {
                offset: 40,
                bytes: MemcmpEncodedBytes::Base58(bs58::encode(&status).into_string()),
                encoding: None,
            }),
        ];

        let incoming_filter = vec![
            RpcFilterType::Memcmp(Memcmp {
                offset: 40,
                bytes: MemcmpEncodedBytes::Base58(bs58::encode(&status).into_string()),
                encoding: None,
            }),
            RpcFilterType::Memcmp(Memcmp {
                offset: 41,
                bytes: MemcmpEncodedBytes::Base58(payer.to_string()),
                encoding: None,
            }),
        ];

        let outgoing = self
            .program
            .accounts(outgoing_filter)
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;
        let incoming = self
            .program
            .accounts(incoming_filter)
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(FriendsError::from(code))
                }
                _ => anyhow!(e),
            })?;

        Ok((outgoing, incoming))
    }

    pub fn list_requests(&self) -> anyhow::Result<RequestList> {
        let (mut outgoing, incoming) = self.list_all()?;
        outgoing.extend(incoming);
        Ok(outgoing)
    }

    fn compute_account_keys(&self, to: Pubkey) -> anyhow::Result<(Pubkey, Pubkey, Pubkey)> {
        self.compute_account_keys_direct(self.program.payer(), to)
    }

    #[allow(clippy::unnecessary_sort_by)]
    fn compute_account_keys_direct(
        &self,
        from: Pubkey,
        to: Pubkey,
    ) -> anyhow::Result<(Pubkey, Pubkey, Pubkey)> {
        let mut list = vec![from, to];
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

#[derive(Clone, Debug, PartialEq)]
pub struct DirectFriendRequest {
    pub from: Pubkey,
    pub status: DirectStatus,
    pub to: Pubkey,
    pub payer: Pubkey,
    pub from_encrypted_key: String,
    pub to_encrypted_key: String,
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum DirectStatus {
    Uninitilized,
    Pending,
    Accepted,
    Denied,
    RemovedFriend,
    RequestRemoved,
}

impl From<Status> for DirectStatus {
    fn from(status: Status) -> Self {
        match status {
            Status::Uninitilized => DirectStatus::Uninitilized,
            Status::Pending => DirectStatus::Pending,
            Status::Accepted => DirectStatus::Accepted,
            Status::Denied => DirectStatus::Denied,
            Status::RemovedFriend => DirectStatus::RequestRemoved,
            Status::RequestRemoved => DirectStatus::RequestRemoved,
        }
    }
}

impl From<DirectStatus> for Status {
    fn from(status: DirectStatus) -> Self {
        match status {
            DirectStatus::Uninitilized => Status::Uninitilized,
            DirectStatus::Pending => Status::Pending,
            DirectStatus::Accepted => Status::Accepted,
            DirectStatus::Denied => Status::Denied,
            DirectStatus::RemovedFriend => Status::RemovedFriend,
            DirectStatus::RequestRemoved => Status::RequestRemoved,
        }
    }
}

impl From<FriendRequest> for DirectFriendRequest {
    fn from(request: FriendRequest) -> Self {
        DirectFriendRequest {
            from: request.from,
            status: DirectStatus::from(request.status),
            to: request.to,
            payer: request.payer,
            from_encrypted_key: request.from_encrypted_key,
            to_encrypted_key: request.to_encrypted_key,
        }
    }
}

impl From<&FriendRequest> for DirectFriendRequest {
    fn from(request: &FriendRequest) -> Self {
        DirectFriendRequest {
            from: request.from,
            status: DirectStatus::from(request.status),
            to: request.to,
            payer: request.payer,
            from_encrypted_key: request.from_encrypted_key.clone(),
            to_encrypted_key: request.to_encrypted_key.clone(),
        }
    }
}

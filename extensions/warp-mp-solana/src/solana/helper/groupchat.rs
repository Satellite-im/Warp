use crate::solana::anchor_client::anchor_lang::prelude::{ProgramError, Pubkey};
use crate::solana::error::GroupError;
use crate::solana::manager::SolanaManager;
use crate::solana::wallet::SolanaWallet;
use anchor_client::solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::{Client, ClientError, Cluster, Program};
use anyhow::{anyhow, bail};
pub use groupchats::{Group, Invitation};
use std::rc::Rc;
use warp::crypto::exchange::X25519PublicKey;

#[allow(unused)]
pub struct GroupChat {
    client: Client,
    program: Program,
    kp: Keypair,
}

#[allow(unused)]
impl GroupChat {
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

        let program = client.program(groupchats::id());
        Self {
            client,
            program,
            kp,
        }
    }

    pub fn create_group(&self, id: &str, name: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();
        let hash = self.group_hash(id);
        let group_key = self.group_address_from_id(id)?;
        let invite_key = self.invite_pubkey(payer, group_key)?;

        let crypto_key = warp::crypto::generate(16);

        let invite = self.encrypt_invite(Invitation {
            sender: payer,
            group_key,
            recipient: payer,
            group_id: id.to_string(),
            encryption_key: hex::encode(crypto_key),
            db_type: 0,
        })?;

        self.program
            .request()
            .signer(&self.kp)
            .accounts(groupchats::accounts::Create {
                group: group_key,
                invitation: invite_key,
                signer: payer,
                payer,
                system_program: anchor_client::solana_sdk::system_program::ID,
            })
            .args(groupchats::instruction::Create {
                _group_hash: hash
                    .try_into()
                    .map_err(|_| anyhow!("Invalid length of hash"))?,
                group_id: invite.group_id,
                open_invites: true,
                name: name.to_string(),
                encryption_key: invite.encryption_key,
                db_type: 1,
            })
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    fn encrypt_invite(&self, invitation: Invitation) -> anyhow::Result<Invitation> {
        let Invitation {
            recipient,
            encryption_key,
            group_id,
            ..
        } = invitation;
        let kp = warp::crypto::signature::Ed25519Keypair::from_bytes(&self.kp.to_bytes())?;
        let secret = warp::crypto::exchange::X25519Secret::from_ed25519_keypair(&kp)?;

        let dh_key = secret.key_exchange(X25519PublicKey::from_bytes(&recipient.to_bytes()));

        let group_id = warp::crypto::cipher::aes256gcm_encrypt(&dh_key, group_id.as_bytes())
            .map(hex::encode)?;

        Ok(Invitation {
            group_id,
            encryption_key: hex::encode(dh_key),
            ..invitation
        })
    }

    fn decrypt_invite(&self, invitation: Invitation) -> anyhow::Result<Invitation> {
        let Invitation {
            sender,
            encryption_key,
            group_id,
            ..
        } = invitation;
        let group_id = hex::decode(group_id)?;
        let kp = warp::crypto::signature::Ed25519Keypair::from_bytes(&self.kp.to_bytes())?;
        let secret = warp::crypto::exchange::X25519Secret::from_ed25519_keypair(&kp)?;

        let dh_key = secret.key_exchange(X25519PublicKey::from_bytes(&sender.to_bytes()));

        let group_id = warp::crypto::cipher::aes256gcm_decrypt(&dh_key, &group_id)?;
        let group_id = String::from_utf8_lossy(&group_id).to_string();

        Ok(Invitation {
            group_id,
            encryption_key: hex::encode(dh_key),
            ..invitation
        })
    }

    pub fn invite_to_group(&self, id: &str, recipient: Pubkey) -> anyhow::Result<()> {
        let payer = self.program.payer();
        let hash = self.group_hash(id);
        let group_key = self.group_address_from_id(id)?;
        let inviter = self.invite_pubkey(payer, group_key)?;
        let invitee = self.invite_pubkey(recipient, group_key)?;

        let creator_invite = self.get_invite_by_group_id(id)?;

        let encrypted = self.encrypt_invite(Invitation {
            recipient,
            ..creator_invite
        })?;

        self.program
            .request()
            .signer(&self.kp)
            .accounts(groupchats::accounts::Invite {
                group: group_key,
                new_invitation: invitee,
                invitation: inviter,
                signer: payer,
                payer,
                system_program: anchor_client::solana_sdk::system_program::ID,
            })
            .args(groupchats::instruction::Invite {
                group_id: id.to_string(),
                recipient,
                encryption_key: encrypted.encryption_key,
                db_type: 0,
            })
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    pub fn modify_name(&self, group: &str, name: &str) -> anyhow::Result<()> {
        let admin = self.program.payer();
        let group = self.group_address_from_id(group)?;
        let name = name.to_string();
        self.program
            .request()
            .signer(&self.kp)
            .accounts(groupchats::accounts::ModifyParameter { group, admin })
            .args(groupchats::instruction::ModifyName { name })
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    pub fn modify_successor(&self, group: &str, receipient: Pubkey) -> anyhow::Result<()> {
        let admin = self.program.payer();
        let group_key = self.group_address_from_id(group)?;
        let group = self.get_group(group_key)?;
        let successor = self.invite_pubkey(receipient, group_key)?;
        self.program
            .request()
            .signer(&self.kp)
            .accounts(groupchats::accounts::ModifySuccessor {
                group: group_key,
                successor,
                admin: group.admin,
            })
            .args(groupchats::instruction::ModifySuccessor)
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    pub fn modify_open_invites(&self, group: &str, open_invites: bool) -> anyhow::Result<()> {
        let group = self.group_address_from_id(group)?;
        let admin = self.program.payer();
        self.program
            .request()
            .signer(&self.kp)
            .accounts(groupchats::accounts::ModifyParameter { group, admin })
            .args(groupchats::instruction::ModifyOpenIvites { open_invites })
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    pub fn admin_leave(&self, group: &str, receipient: Pubkey) -> anyhow::Result<()> {
        let payer = self.program.payer();
        let group_key = self.group_address_from_id(group)?;
        let inviter = self.invite_pubkey(payer, group_key)?;
        let successor = self.invite_pubkey(receipient, group_key)?;
        self.program
            .request()
            .signer(&self.kp)
            .accounts(groupchats::accounts::AdminLeave {
                group: group_key,
                invitation: inviter,
                signer: payer,
                invitation_sender: payer,
                successor,
            })
            .args(groupchats::instruction::AdminLeave)
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    pub fn leave(&self, group: &str) -> anyhow::Result<()> {
        let payer = self.program.payer();
        let group_key = self.group_address_from_id(group)?;
        let group = self.get_group(group_key)?;
        let invite = self.invite_pubkey(payer, group_key)?;
        self.program
            .request()
            .signer(&self.kp)
            .accounts(groupchats::accounts::Leave {
                group: group_key,
                invitation: invite,
                signer: payer,
                invitation_sender: group.admin,
            })
            .args(groupchats::instruction::Leave)
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    pub fn close(&self, group: &str) -> anyhow::Result<()> {
        let group_key = self.group_address_from_id(group)?;
        let group = self.get_group(group_key)?;
        let payer = self.program.payer();
        let invitation = self.invite_pubkey(payer, group_key)?;
        self.program
            .request()
            .signer(&self.kp)
            .accounts(groupchats::accounts::Close {
                group: group_key,
                invitation,
                signer: payer,
                creator: group.creator,
                invitation_sender: group.admin,
            })
            .args(groupchats::instruction::Close)
            .send()
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?;
        Ok(())
    }

    fn group_pubkey(&self, hash: &[u8]) -> anyhow::Result<(Pubkey, u8)> {
        Pubkey::try_find_program_address(&[hash, b"groupchat"], &self.program.id())
            .ok_or_else(|| anyhow!("Error finding program"))
    }

    fn invite_pubkey(&self, user: Pubkey, group: Pubkey) -> anyhow::Result<Pubkey> {
        let (key, _) = Pubkey::try_find_program_address(
            &[&user.to_bytes(), &group.to_bytes(), b"invite"],
            &self.program.id(),
        )
        .ok_or_else(|| anyhow!("Error finding program"))?;
        Ok(key)
    }

    fn group_hash(&self, id: &str) -> Vec<u8> {
        warp::crypto::hash::sha256_hash(id.as_bytes(), None)
    }

    pub fn group_address_from_id(&self, id: &str) -> anyhow::Result<Pubkey> {
        let hash = self.group_hash(id);
        let (key, _) = self.group_pubkey(&hash)?;
        Ok(key)
    }

    pub fn get_invitation(&self, key: Pubkey) -> anyhow::Result<Invitation> {
        let account = self.program.account(key)?;
        Ok(account)
    }

    pub fn get_invitation_accounts(
        &self,
        filter: Vec<InvitationAccountFilter>,
    ) -> anyhow::Result<Vec<Invitation>> {
        println!("{:?}", filter);

        let filter = filter
            .iter()
            .map(|invite| RpcFilterType::Memcmp(invite.to_memcmp()))
            .collect::<Vec<_>>();

        let list = self
            .program
            .accounts(filter)
            .map_err(|e| match e {
                ClientError::ProgramError(ProgramError::Custom(code)) => {
                    anyhow!(GroupError::from(code))
                }
                _ => anyhow!(e),
            })?
            .iter()
            .map(|(_, inv)| inv)
            .cloned()
            .collect::<Vec<_>>();

        Ok(list)
    }

    fn get_invite_by_group_id(&self, id: &str) -> anyhow::Result<Invitation> {
        let group_key = self.group_address_from_id(id)?;

        let recipient = self.program.payer();

        let invite = self.get_invitation_accounts(vec![
            InvitationAccountFilter::GroupKey(group_key),
            InvitationAccountFilter::Recipient(recipient),
        ])?;
        if let Some(invite) = invite.first().cloned() {
            return self.decrypt_invite(invite);
        }
        bail!("No invitation found");
    }

    pub fn get_group(&self, addr: Pubkey) -> anyhow::Result<Group> {
        let group = self.program.account(addr)?;
        Ok(group)
    }

    pub fn get_user_groups(&self, addr: Pubkey) -> anyhow::Result<Vec<Group>> {
        unimplemented!()
    }
}

// #[derive(Clone, Debug, PartialEq, Eq)]
// pub struct DirectInvitation {
//     pub sender: Pubkey,
//     pub group_key: Pubkey,
//     pub recipient: Pubkey,
//     pub group_id: String,
//     pub encryption_key: String,
//     pub db_type: u8,
// }
//
// impl DirectInvitation {
//     pub fn from_invite(invite: &Invitation) -> Self {
//         DirectInvitation {
//             sender: invite.sender,
//             group_key: invite.group_key,
//             recipient: invite.recipient,
//             group_id: invite.group_id.clone(),
//             encryption_key: invite.encryption_key.clone(),
//             db_type: invite.db_type,
//         }
//     }
// }

// For compat for wasm, though this may not be used
// #[derive(Debug, Clone, PartialEq, Eq)]
// pub struct InvitationAccountFilter(InvitationAccountFilterInner);
//
// impl InvitationAccountFilter {
//     pub fn recipient(key: Pubkey) -> Self {
//         InvitationAccountFilter(InvitationAccountFilterInner::Recipient(key))
//     }
//
//     pub fn sender(key: Pubkey) -> Self {
//         InvitationAccountFilter(InvitationAccountFilterInner::Sender(key))
//     }
//
//     pub fn group_key(key: Pubkey) -> Self {
//         InvitationAccountFilter(InvitationAccountFilterInner::GroupKey(key))
//     }
// }
//
// impl AsRef<InvitationAccountFilterInner> for InvitationAccountFilter {
//     fn as_ref(&self) -> &InvitationAccountFilterInner {
//         &self.0
//     }
// }

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum InvitationAccountFilter {
    Recipient(Pubkey),
    Sender(Pubkey),
    GroupKey(Pubkey),
}

impl InvitationAccountFilter {
    pub fn to_offset(&self) -> usize {
        match self {
            Self::Sender(_) => 8,
            Self::GroupKey(_) => 40,
            Self::Recipient(_) => 72,
        }
    }

    pub fn to_key(&self) -> Pubkey {
        match self {
            Self::Sender(key) => *key,
            Self::GroupKey(key) => *key,
            Self::Recipient(key) => *key,
        }
    }

    pub fn to_memcmp(&self) -> Memcmp {
        Memcmp {
            offset: self.to_offset(),
            bytes: MemcmpEncodedBytes::Base58(self.to_key().to_string()),
            encoding: None,
        }
    }
}

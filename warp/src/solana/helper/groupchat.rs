use crate::crypto::x25519_dalek::PublicKey;
use crate::solana::manager::SolanaManager;
use crate::solana::wallet::SolanaWallet;
use anchor_client::anchor_lang::prelude::Pubkey;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::{Client, Cluster, Program};
use anyhow::anyhow;
use groupchats::{Group, Invitation};
use std::rc::Rc;

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

        let crypto_key = crate::crypto::generate(16);

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
                group_id: id.to_string(),
                open_invites: true,
                name: name.to_string(),
                encryption_key: invite.encryption_key,
                db_type: 1,
            })
            .send()?;
        Ok(())
    }

    fn encrypt_invite(&self, invitation: Invitation) -> anyhow::Result<Invitation> {
        let Invitation {
            recipient,
            encryption_key,
            group_id,
            ..
        } = invitation;
        let kp = ed25519_dalek::Keypair::from_bytes(&self.kp.to_bytes())?;
        let secret = crate::crypto::exchange::ed25519_to_x25519(&kp);

        let dh_key = crate::crypto::exchange::x25519_key_exchange(
            &secret,
            PublicKey::from(recipient.to_bytes()),
            None,
            true,
        );

        let group_id = crate::crypto::cipher::aes256gcm_encrypt(&dh_key, group_id.as_bytes())
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
        let kp = crate::crypto::ed25519_dalek::Keypair::from_bytes(&self.kp.to_bytes())?;
        let secret = crate::crypto::exchange::ed25519_to_x25519(&kp);

        let dh_key = crate::crypto::exchange::x25519_key_exchange(
            &secret,
            PublicKey::from(sender.to_bytes()),
            None,
            true,
        );

        let group_id = crate::crypto::cipher::aes256gcm_decrypt(&dh_key, &group_id)?;
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
                encryption_key: String::new(),
                db_type: 1,
            })
            .send()?;
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
            .send()?;
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
            .send()?;
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
            .send()?;
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
            .send()?;
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
            .send()?;
        Ok(())
    }

    //TODO
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
            .send()?;
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
        crate::crypto::hash::sha256_hash(id.as_bytes(), None)
    }

    fn group_address_from_id(&self, id: &str) -> anyhow::Result<Pubkey> {
        let hash = self.group_hash(id);
        let (key, _) = self.group_pubkey(&hash)?;
        Ok(key)
    }

    pub fn get_invitation(&self, key: Pubkey) -> anyhow::Result<Invitation> {
        let account = self.program.account(key)?;
        Ok(account)
    }

    pub fn get_invitation_accounts(&self) -> anyhow::Result<Vec<Invitation>> {
        let list = self
            .program
            .accounts(vec![])?
            .iter()
            .map(|(_, inv)| inv)
            .cloned()
            .collect::<Vec<Invitation>>();

        Ok(list)
    }

    fn get_invite_by_group_id(&self, id: &str) -> anyhow::Result<Invitation> {
        unimplemented!()
    }

    pub fn get_group(&self, addr: Pubkey) -> anyhow::Result<Group> {
        let group = self.program.account(addr)?;
        Ok(group)
    }

    pub fn get_user_groups(&self, addr: Pubkey) -> anyhow::Result<Vec<Group>> {
        unimplemented!()
    }
}

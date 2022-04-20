use crate::manager::SolanaManager;
use crate::wallet::SolanaWallet;
use crate::Pubkey;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::signature::Keypair;
use anchor_client::{Client, Cluster, Program};
use groupchats::{Group, Invitation};
use std::rc::Rc;
use warp_common::anyhow::anyhow;
use warp_common::{anyhow, hex};
use warp_crypto::x25519_dalek::PublicKey;

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

        let crypto_key = warp_crypto::generate(16);

        let invite = self.encrypt_invite(Invitation {
            sender: payer,
            group_key,
            recipient: payer,
            group_id: id.to_string(),
            encryption_key: hex::encode(crypto_key),
            db_type: 0,
        });

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
                encryption_key: String::new(),
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
        let kp = warp_crypto::ed25519_dalek::Keypair::from_bytes(&self.kp.to_bytes())?;
        let secret = warp_crypto::exchange::ed25519_to_x25519(&kp);

        let dh_key = warp_crypto::exchange::x25519_key_exchange(
            &secret,
            PublicKey::from(recipient.to_bytes()),
            None,
            true,
        );

        let group_id = warp_crypto::cipher::aes256gcm_encrypt(&dh_key, group_id.as_bytes())
            .map(warp_common::hex::encode)?;

        Ok(Invitation {
            group_id,
            encryption_key: warp_common::hex::encode(dh_key),
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
        let kp = warp_crypto::ed25519_dalek::Keypair::from_bytes(&self.kp.to_bytes())?;
        let secret = warp_crypto::exchange::ed25519_to_x25519(&kp);

        let dh_key = warp_crypto::exchange::x25519_key_exchange(
            &secret,
            PublicKey::from(sender.to_bytes()),
            None,
            true,
        );

        let group_id = warp_crypto::cipher::aes256gcm_decrypt(&dh_key, &group_id)?;
        let group_id = String::from_utf8_lossy(&group_id).to_string();

        Ok(Invitation {
            group_id,
            encryption_key: warp_common::hex::encode(dh_key),
            ..invitation
        })
    }

    pub fn invite_to_group(&self, id: &str, recipient: &Pubkey) -> anyhow::Result<()> {
        let payer = self.program.payer();
        let hash = self.group_hash(id);
        let group_key = self.group_address_from_id(id)?;
        let inviter = self.invite_pubkey(payer, group_key)?;
        let invitee = self.invite_pubkey(*recipient, group_key)?;

        let creator_invite = self.get_invite_by_group_id(id)?;

        let encrypted = self.encrypt_invite(Invitation {
            recipient: *recipient,
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
                recipient: *recipient,
                encryption_key: String::new(),
                db_type: 1,
            })
            .send()?;
        Ok(())
    }

    fn group_pubkey(&self, hash: &[u8]) -> anyhow::Result<(Pubkey, u8)> {
        Pubkey::try_find_program_address(&[hash, b"groupchat"], &self.program.id())
            .ok_or_else(|| anyhow!("Error finding program"))
    }

    fn invite_pubkey(&self, user: Pubkey, group: Pubkey) -> anyhow::Result<Pubkey> {
        let (key, _) = Pubkey::try_find_program_address(
            &[&user.to_bytes(), &group.to_bytes(), b"groupchat"],
            &self.program.id(),
        )
        .ok_or_else(|| anyhow!("Error finding program"))?;
        Ok(key)
    }

    fn group_hash(&self, id: &str) -> Vec<u8> {
        warp_crypto::hash::sha256_hash(id.as_bytes(), None)
    }

    fn group_address_from_id(&self, id: &str) -> anyhow::Result<Pubkey> {
        let hash = self.group_hash(id);
        let (key, _) = self.group_pubkey(&hash)?;
        Ok(key)
    }

    fn get_invitation_accounts(&self) -> anyhow::Result<Vec<Invitation>> {
        unimplemented!()
    }

    fn get_invite_by_group_id(&self, id: &str) -> anyhow::Result<Invitation> {
        unimplemented!()
    }

    pub fn get_user_groups(&self, addr: Pubkey) -> anyhow::Result<Vec<Group>> {
        unimplemented!()
    }
}

use std::sync::{Arc, Mutex};

use crate::anyhow::ensure;
use warp_common::anyhow::{anyhow, bail};
use warp_common::error::Error;
use warp_common::{anyhow, Extension, Module};
use warp_crypto::rand::Rng;
use warp_data::DataType;
use warp_hooks::hooks::Hooks;
use warp_multipass::{identity::*, MultiPass};
use warp_pocket_dimension::PocketDimension;
use warp_solana_utils::helper::user::UserHelper;
use warp_solana_utils::manager::SolanaManager;
use warp_solana_utils::solana_client::rpc_client::RpcClient;
use warp_solana_utils::solana_sdk::pubkey::Pubkey;
use warp_solana_utils::solana_sdk::signature::Keypair;
use warp_solana_utils::solana_sdk::signer::Signer;
use warp_solana_utils::wallet::{PhraseType, SolanaWallet};
use warp_solana_utils::EndPoint;
use warp_tesseract::Tesseract;

pub struct Account {
    pub endpoint: EndPoint,
    pub connection: Option<RpcClient>,
    pub identity: Option<Identity>,
    pub contacts: Option<Vec<PublicKey>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub tesseract: Option<Arc<Mutex<Tesseract>>>,
    pub hooks: Option<Arc<Mutex<Hooks>>>,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            identity: None,
            connection: None,
            endpoint: EndPoint::DevNet,
            contacts: None,
            cache: None,
            tesseract: None,
            hooks: None,
        }
    }
}

impl Account {
    pub fn new(endpoint: EndPoint) -> Self {
        let mut account = Self::default();
        account.endpoint = endpoint.clone();
        account.connection = Some(RpcClient::new(endpoint.to_string()));
        account
    }

    pub fn with_devnet() -> Self {
        Self::new(EndPoint::DevNet)
    }

    pub fn with_mainnet() -> Self {
        Self::new(EndPoint::MainNetBeta)
    }

    pub fn with_testnet() -> Self {
        Self::new(EndPoint::TestNet)
    }

    pub fn set_cache(&mut self, cache: Arc<Mutex<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hooks: Arc<Mutex<Hooks>>) {
        self.hooks = Some(hooks)
    }

    pub fn set_tesseract(&mut self, tesseract: Arc<Mutex<Tesseract>>) {
        self.tesseract = Some(tesseract)
    }

    pub fn insert_private_key(&mut self, wallet: SolanaWallet) -> anyhow::Result<()> {
        ensure!(self.tesseract.is_some(), "Tesseract is not available");
        let mut tesseract = self.tesseract.as_mut().unwrap().lock().unwrap();

        ensure!(tesseract.is_unlock(), "Tesseract is currently locked.");

        tesseract.set("mnemonic", wallet.mnemonic.as_str())?;
        tesseract.set("privkey", wallet.keypair.to_base58_string().as_str())?;

        Ok(())
    }

    pub fn get_private_key(&self) -> anyhow::Result<Keypair> {
        ensure!(self.tesseract.is_some(), "Tesseract is not available");
        let tesseract = self.tesseract.as_ref().unwrap().lock().unwrap();

        ensure!(tesseract.is_unlock(), "Tesseract is currently locked.");

        if !tesseract.exist("privkey") {
            bail!("Private key is not set or available");
        }

        let private_key = tesseract.retrieve("privkey")?;

        let keypair = Keypair::from_base58_string(private_key.as_str());
        Ok(keypair)
    }

    pub fn user_helper(&self) -> anyhow::Result<UserHelper> {
        let kp = self.get_private_key()?;
        let helper = UserHelper::new_with_keypair(&kp);
        Ok(helper)
    }
}

impl Extension for Account {
    fn id(&self) -> String {
        String::from("warp-mp-solana")
    }

    fn name(&self) -> String {
        String::from("Solana Multipass")
    }

    fn module(&self) -> warp_common::Module {
        Module::Accounts
    }
}

impl MultiPass for Account {
    fn create_identity(&mut self, username: &str, _: &str) -> warp_common::Result<PublicKey> {
        if self.tesseract.is_none() {
            return Err(Error::Any(anyhow!(
                "Tesseract is required to create an identity"
            )));
        }

        if let Ok(keypair) = &self.get_private_key() {
            if UserHelper::new_with_keypair(keypair)
                .get_current_user()
                .is_ok()
            {
                return Err(Error::Other);
            }
        }

        let mut tesseract = self.tesseract.as_mut().unwrap().lock().unwrap();

        let wallet = SolanaWallet::create_random(PhraseType::Standard, None)?;
        let mut helper = UserHelper::new_with_wallet(&wallet);

        if let Ok(identity) = user_to_identity(&helper, None) {
            if identity.username == *username {
                return Err(Error::ToBeDetermined);
            }
        }
        let mut manager = SolanaManager::new();
        manager.initiralize_from_solana_wallet(&wallet)?;
        //
        if manager.get_account_balance()? == 0 {
            manager.request_air_drop()?;
        }
        let code: i32 = warp_crypto::rand::thread_rng().gen_range(0, 9999);

        let uname = format!("{username}#{code}");

        helper.create(&uname, "", "We have liftoff")?;

        tesseract.set("mnemonic", wallet.mnemonic.as_str())?;
        tesseract.set("privkey", wallet.keypair.to_base58_string().as_str())?;

        let pubkey = PublicKey::from_bytes(&wallet.keypair.pubkey().to_bytes()[..]);

        let identity = user_to_identity(&helper, None)?;

        self.identity = Some(identity);
        Ok(pubkey)
    }

    fn get_identity(&self, id: Identifier) -> warp_common::Result<Identity> {
        let helper = self.user_helper()?;
        let ident = match id {
            Identifier::Username(_) => return Err(Error::Unimplemented),
            Identifier::PublicKey(pkey) => user_to_identity(&helper, Some(pkey.to_bytes()))?,
            Identifier::Own => user_to_identity(&helper, None)?,
        };

        Ok(ident)
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> warp_common::Result<()> {
        let mut helper = self.user_helper()?;

        let mut identity = user_to_identity(&helper, None)?;

        match option {
            IdentityUpdate::Username(username) => {
                helper.set_name(&format!("{username}#{}", identity.short_id))?; //TODO: Investigate why it errors and causes interaction to contract to error until there is an update.
                identity.username = username
            }
            IdentityUpdate::Graphics { picture, banner } => {
                if let Some(hash) = picture {
                    helper.set_photo(&hash)?;
                    identity.graphics.profile_picture = hash;
                }
                if let Some(hash) = banner {
                    helper.set_banner_image(&hash)?;
                    identity.graphics.profile_banner = hash;
                }
            }
            IdentityUpdate::StatusMessage(status) => {
                helper.set_status(&status.clone().unwrap_or_default())?;
                identity.status_message = status
            }
        }

        self.identity = Some(identity);
        Ok(())
    }

    fn decrypt_private_key(&self, _: &str) -> warp_common::Result<Vec<u8>> {
        if self.tesseract.is_none() {
            return Err(Error::Any(anyhow!(
                "Tesseract is required to create an identity"
            )));
        }

        let keypair = self.get_private_key()?;

        Ok(keypair.to_bytes().to_vec())
    }

    fn refresh_cache(&mut self) -> warp_common::Result<()> {
        if let Some(cache) = &self.cache {
            let mut cache = cache.lock().unwrap();
            return cache.empty(DataType::Module(self.module()));
        }
        Err(warp_common::error::Error::Any(anyhow!(
            "Cache extension was not enabled"
        )))
    }
}

fn user_to_identity(helper: &UserHelper, pubkey: Option<&[u8]>) -> anyhow::Result<Identity> {
    let user = match pubkey {
        Some(pubkey) => helper.get_user(&Pubkey::new(pubkey))?,
        None => helper.get_current_user()?,
    };
    let mut identity = Identity::default();
    //Note: This is temporary
    if user.name.contains('#') {
        let split_data = user.name.split('#').collect::<Vec<&str>>();

        if split_data.len() != 2 {
            //Because of it being invalid and due to the lack of short code within the contract
            //we will not error here but instead would ignore and return the original username to
            //the identity.
            identity.username = user.name;
        } else {
            match (
                split_data.get(0).ok_or(Error::Other).map(|s| s.to_string()),
                split_data.get(1).ok_or(Error::Other)?.parse(),
            ) {
                (Ok(name), Ok(code)) => {
                    identity.username = name;
                    identity.short_id = code;
                }
                _ => identity.username = user.name,
            };
        }
    } else {
        identity.username = user.name
    };

    identity.public_key = PublicKey::from_bytes(&helper.user_pubkey().to_bytes());
    identity.status_message = Some(user.status);
    identity.graphics.profile_picture = user.photo_hash;
    identity.graphics.profile_banner = user.banner_image_hash;
    Ok(identity)
}

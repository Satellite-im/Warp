use std::sync::{Arc, Mutex};

use warp_common::anyhow::anyhow;
use warp_common::error::Error;
use warp_common::{Extension, Module};
use warp_data::{DataObject, DataType};
use warp_hooks::hooks::Hooks;
use warp_multipass::{identity::*, MultiPass};
use warp_pocket_dimension::PocketDimension;
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
    pub wallet: Option<SolanaWallet>,
    pub contacts: Option<Vec<PublicKey>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub tesseract: Option<Arc<Mutex<Tesseract>>>,
    pub hooks: Option<Arc<Mutex<Hooks>>>,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            identity: None,
            wallet: None,
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
    fn create_identity(
        &mut self,
        identity: &mut Identity,
        passphrase: &str,
    ) -> warp_common::Result<PublicKey> {
        if self.tesseract.is_none() {
            return Err(Error::Any(anyhow!(
                "Tesseract is required to create an identity"
            )));
        }

        let mut tesseract = self.tesseract.as_mut().unwrap().lock().unwrap();

        if tesseract.exist("privkey") {
            let private_key = tesseract.retrieve(passphrase.as_bytes(), "privkey")?;

            let keypair = Keypair::from_base58_string(private_key.as_str());

            let pubkey = Pubkey::new(identity.public_key.to_bytes());

            if keypair.pubkey() == pubkey {
                return Err(Error::Other);
            }
        }

        let wallet = SolanaWallet::create_random(PhraseType::Standard, None)?;
        self.wallet = Some(wallet.clone());

        //TODO: Solana implementation here for creating an account on the blockchain

        // The phrase or mnemonic is set in the event we need to show it to the user
        tesseract.set(passphrase.as_bytes(), "mnemonic", wallet.mnemonic.as_str())?;
        tesseract.set(
            passphrase.as_bytes(),
            "privkey",
            wallet.keypair.to_base58_string().as_str(),
        )?;

        let pubkey = PublicKey::from_bytes(&wallet.keypair.pubkey().to_bytes()[..]);
        identity.public_key = pubkey.clone();

        self.identity = Some(identity.clone());
        Ok(pubkey)
    }

    fn get_identity(&self, id: Identifier) -> warp_common::Result<DataObject> {
        let identity = match id {
            Identifier::Username(_) => return Err(Error::Unimplemented),
            Identifier::PublicKey(_) => return Err(Error::Unimplemented),
            Identifier::Own => self.identity.as_ref().unwrap(),
        };

        DataObject::new(&DataType::Module(Module::Accounts), identity)
    }

    fn update_identity(
        &mut self,
        id: Identifier,
        option: Vec<IdentityUpdate>,
    ) -> warp_common::Result<()> {
        let mut identity = match id {
            Identifier::Username(_) => return Err(Error::Unimplemented),
            Identifier::PublicKey(_) => return Err(Error::Unimplemented),
            Identifier::Own => self
                .identity
                .as_mut()
                .ok_or_else(|| anyhow!("Identity is not defined"))?,
        };

        for option in option {
            match option {
                IdentityUpdate::Username(username) => identity.username = username,
                IdentityUpdate::Graphics { picture, banner } => {
                    if let Some(hash) = picture {
                        identity.graphics.profile_picture = hash;
                    }
                    if let Some(hash) = banner {
                        identity.graphics.profile_banner = hash;
                    }
                }
                IdentityUpdate::StatusMessage(status) => identity.status_message = status,
            }
        }

        self.identity = Some(identity.clone());
        Ok(())
    }

    fn decrypt_private_key(&self, passphrase: &str) -> warp_common::Result<Vec<u8>> {
        if self.tesseract.is_none() {
            return Err(Error::Any(anyhow!(
                "Tesseract is required to create an identity"
            )));
        }

        let tesseract = self.tesseract.as_ref().unwrap().lock().unwrap();

        if !tesseract.exist("privkey") {
            return Err(Error::Any(anyhow!("Private key is not set or available")));
        }

        let private_key = tesseract.retrieve(passphrase.as_bytes(), "privkey")?;

        let keypair = Keypair::from_base58_string(private_key.as_str());

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

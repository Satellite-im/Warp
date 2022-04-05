use std::sync::{Arc, Mutex};

use warp_common::anyhow::{anyhow, ensure};
use warp_common::error::Error;
use warp_common::solana_client::rpc_client::RpcClient;
use warp_common::solana_sdk::pubkey::Pubkey;
use warp_common::solana_sdk::signature::Keypair;
use warp_common::solana_sdk::signer::Signer;
use warp_common::{Extension, Module};
use warp_data::{DataObject, DataType};
use warp_hooks::hooks::Hooks;
use warp_multipass::{identity::*, MultiPass};
use warp_pocket_dimension::PocketDimension;
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

    //TODO: Use when creating identity
    pub fn generate(&mut self) -> warp_common::anyhow::Result<()> {
        ensure!(self.wallet.is_none(), "Account already exist"); //TODO: Perform a better check
        let wallet =
            SolanaWallet::create_random(warp_solana_utils::wallet::PhraseType::Standard, None)?;
        self.wallet = Some(wallet);
        Ok(())
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

        let mut tesseract = self.tesseract.as_ref().unwrap().lock().unwrap();

        if tesseract.exist("privkey") {
            let private_key = tesseract.retrieve(passphrase.as_bytes(), "privkey")?;

            let keypair = Keypair::from_base58_string(private_key.as_str());

            let pubkey = Pubkey::new(identity.public_key.to_bytes());

            if keypair.pubkey() == pubkey {
                return Err(Error::Other);
            }
        }

        let wallet = SolanaWallet::create_random(PhraseType::Standard, None)?;

        //TODO: Solana implementation here for creating an account on the blockchain

        tesseract.set(passphrase.as_bytes(), "mnemonic", wallet.mnemonic.as_str())?;
        tesseract.set(
            passphrase.as_bytes(),
            "privkey",
            wallet.keypair.to_base58_string().as_str(),
        )?;

        let pubkey = PublicKey::from_bytes(&wallet.keypair.pubkey().to_bytes()[..]);
        identity.public_key = pubkey.clone();

        Ok(pubkey)
    }

    fn get_identity(&self, id: Identifier) -> warp_common::Result<DataObject> {
        match id {
            Identifier::Username(_) => Err(Error::Unimplemented),
            Identifier::PublicKey(_) => Err(Error::Unimplemented),
            Identifier::Own => DataObject::new(
                &DataType::Module(Module::Accounts),
                self.identity.as_ref().unwrap(),
            ),
        }
    }

    fn update_identity(
        &mut self,
        _id: Identifier,
        _option: Vec<IdentityUpdate>,
    ) -> warp_common::Result<()> {
        todo!()
    }

    fn decrypt_private_key(&self, _passphrase: &str) -> warp_common::Result<Vec<u8>> {
        todo!()
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

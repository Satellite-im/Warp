use std::sync::{Arc, Mutex};

use warp_common::anyhow::{anyhow, ensure};
use warp_common::error::Error;
use warp_common::solana_client::rpc_client::RpcClient;
use warp_common::{Extension, Module};
use warp_data::{DataObject, DataType};
use warp_hooks::hooks::Hooks;
use warp_multipass::{identity::*, MultiPass};
use warp_pocket_dimension::PocketDimension;
use warp_solana_utils::wallet::SolanaWallet;
use warp_solana_utils::EndPoint;

pub struct Account {
    pub endpoint: EndPoint,
    pub connection: Option<RpcClient>,
    pub wallet: Option<SolanaWallet>,
    pub contacts: Option<Vec<PublicKey>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub hooks: Option<Arc<Mutex<Hooks>>>,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            wallet: None,
            connection: None,
            endpoint: EndPoint::DevNet,
            contacts: None,
            cache: None,
            hooks: None,
        }
    }
}

impl Account {
    pub fn new(endpoint: EndPoint) -> Self {
        let mut account = Self::default();
        account.endpoint = endpoint.clone();
        account.connection = Some(RpcClient::new(endpoint));
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

    //TODO: Use when creating identit
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
        _identity: &Identity,
        _passphrase: &str,
    ) -> warp_common::Result<PublicKey> {
        todo!()
    }

    fn get_identity(&self, id: Identifier) -> warp_common::Result<DataObject> {
        match id {
            Identifier::Username(_) => return Err(Error::Unimplemented),
            Identifier::PublicKey(_) => {}
            Identifier::Own => {}
        }

        unimplemented!()
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

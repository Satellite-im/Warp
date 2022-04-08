use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::keypair_from_seed;
use anchor_client::solana_sdk::signature::{Keypair, Signer};
use std::collections::HashMap;
use warp_common::anyhow::{ensure, Result};
use warp_common::bip39::{Language, Mnemonic, Seed};

use crate::wallet::SolanaWallet;
use crate::{anyhow, EndPoint};

pub struct SolanaManager {
    pub accounts: Vec<SolanaWallet>,
    pub payer_account: Option<Keypair>,
    pub mnemonic: Option<String>,
    pub connection: RpcClient,
    pub user_account: Option<Keypair>,
    pub network_identifier: EndPoint,
    pub cluster_endpoint: EndPoint,
    pub public_keys: HashMap<String, Pubkey>,
}

impl Default for SolanaManager {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            payer_account: None,
            mnemonic: None,
            connection: RpcClient::new(EndPoint::DevNet.to_string()),
            user_account: None,
            network_identifier: EndPoint::DevNet,
            cluster_endpoint: EndPoint::DevNet,
            public_keys: HashMap::new(),
        }
    }
}

impl std::fmt::Debug for SolanaManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaManager")
            .field("accounts", &self.accounts)
            .field("payer_account", &self.payer_account)
            .field("mnemonic", &self.mnemonic)
            .field("connection", &"<>")
            .field("user_account", &self.user_account)
            .field("network_identifier", &self.network_identifier)
            .field("cluster_endpoint", &self.cluster_endpoint)
            .field("public_keys", &self.public_keys)
            .finish()
    }
}

impl SolanaManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_from_endpoint(endpoint: EndPoint) -> Self {
        let connection = RpcClient::new(endpoint.to_string());
        Self {
            accounts: Vec::new(),
            payer_account: None,
            mnemonic: None,
            connection,
            user_account: None,
            network_identifier: endpoint.clone(),
            cluster_endpoint: endpoint,
            public_keys: HashMap::new(),
        }
    }

    //not used
    pub fn generate_user_keypair(&self) -> Result<Keypair> {
        let mnemonic = self
            .mnemonic
            .as_ref()
            .ok_or_else(|| anyhow!("Mnemonic phrase is invalid"))?;
        let mnemonic = Mnemonic::from_phrase(mnemonic.as_str(), Language::English)?;
        let seed = Seed::new(&mnemonic, "");
        // let hash = sha256_hash(&format!("{}user", String::from_utf8_lossy(seed.as_bytes())));
        let keypair = keypair_from_seed(seed.as_bytes()).map_err(|e| anyhow!(e.to_string()))?;
        // let keypair = keypair_from_seed(hash.as_bytes()).map_err(|e| anyhow!(e.to_string()))?;
        Ok(keypair)
    }

    // pub fn generate_derived_public_key(
    //     &mut self,
    //     identifier: &str,
    //     user_pkey: &Pubkey,
    //     seed: &str,
    //     id: &Pubkey,
    // ) -> Result<Pubkey> {
    //     let (_, pkey) = crate::pubkey_from_seed(user_pkey, seed, id)?;
    //     self.public_keys.insert(identifier.to_string(), pkey);
    //     Ok(pkey)
    // }

    //Not needed?
    // pub fn get_derived_pubkey(&self, identifier: &str) -> Result<&Pubkey> {
    //     self.public_keys
    //         .get(identifier)
    //         .ok_or(Error::ToBeDetermined)
    //         .map_err(|e| anyhow!(e))
    // }

    //Not needed?
    // pub fn generate_new_account(&mut self) -> Result<SolanaWallet> {
    //     let mnemonic = self.mnemonic.as_ref().ok_or(Error::ToBeDetermined)?;
    //     let account = SolanaWallet::restore_keypair_from_mnemonic(
    //         mnemonic.as_str(),
    //         self.accounts.len() as u16,
    //     )?;
    //     //TODO: Change to borrow or clone `SolanaWallet`
    //     self.accounts
    //         .push(SolanaWallet::restore_keypair_from_mnemonic(
    //             mnemonic.as_str(),
    //             self.accounts.len() as u16,
    //         )?);
    //
    //     Ok(account)
    // }

    //Not needed?
    // pub fn initialize_random(&mut self) -> Result<()> {
    //     let account = SolanaWallet::create_random_keypair()?;
    //     self.payer_account = Some(account.get_keypair()?);
    //     self.mnemonic = Some(account.mnemonic.clone());
    //     self.user_account = Some(self.generate_user_keypair()?);
    //     self.accounts.push(account);
    //     Ok(())
    // }

    pub fn initialize_from_mnemonic(&mut self, mnemonic: &str) -> Result<()> {
        let wallet = SolanaWallet::restore_from_mnemonic(None, mnemonic)?;
        self.initiralize_from_solana_wallet(wallet)
    }

    pub fn initiralize_from_solana_wallet(&mut self, wallet: SolanaWallet) -> Result<()> {
        self.payer_account = Some({
            let inner = wallet.keypair.to_bytes();
            Keypair::from_bytes(&inner)?
        });
        self.mnemonic = Some(wallet.mnemonic.clone());
        // self.user_account = Some(self.generate_user_keypair()?);
        self.accounts.push(wallet); //TODO: Borrow or clone but why dup?
        Ok(())
    }

    // pub fn get_account(&self, address: &str) -> Result<&SolanaWallet> {
    //     if self
    //         .accounts
    //         .iter()
    //         .filter(|wallet| wallet.address.as_str() == address)
    //         .count()
    //         >= 2
    //     {
    //         bail!("Duplication of address provided")
    //     }
    //     let wallets = self
    //         .accounts
    //         .iter()
    //         .filter(|wallet| wallet.address.as_str() == address)
    //         .collect::<Vec<_>>();
    //     ensure!(wallets.len() == 1, "Address provided is invalid");
    //     let wallet = wallets.get(0).ok_or(Error::ToBeDetermined)?;
    //     Ok(wallet)
    // }

    // pub fn list(&self) -> &Vec<SolanaWallet> {
    //     &self.accounts
    // }
    //
    // pub fn get_user_account(&self) -> Result<&Keypair> {
    //     let account = self.user_account.as_ref().ok_or(Error::ToBeDetermined)?;
    //     Ok(account)
    // }

    pub fn get_account_balance(&self) -> Result<u64> {
        ensure!(self.payer_account.is_some(), "Invalid payer account");
        let payer_account = self.payer_account.as_ref().unwrap();
        let commitment_config = CommitmentConfig::confirmed();
        let result = self
            .connection
            .get_balance_with_commitment(&payer_account.pubkey(), commitment_config)?;
        Ok(result.value)
    }

    // TODO: Determine if needed
    // Note: This function would continuously check for the account until it becomes available.
    // One solution outside utilizing futures would be performing a check on a separate thread and use channels to communicate when it is found
    // Another option would be to utilize a future and wait for it to return a result. Last option would be to allow for this to stall on the main
    // thread until it is completed. Possibly implement a timeout to prevent the loop from continuing forever.
    pub fn wait_for_account(&self) -> Result<()> {
        todo!()
    }
}

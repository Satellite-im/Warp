use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::keypair_from_seed;
use anchor_client::solana_sdk::signature::{Keypair, Signer};
use std::collections::HashMap;
use warp_common::anyhow::{ensure, Result};
use warp_common::bip39::{Language, Mnemonic, Seed};

use crate::wallet::{PhraseType, SolanaWallet};
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

//Note: We should not be cloning the manager
impl Clone for SolanaManager {
    fn clone(&self) -> Self {
        Self {
            accounts: self.accounts.clone(),
            payer_account: match &self.payer_account {
                Some(kp) => {
                    let inner = kp.to_base58_string();
                    Some(Keypair::from_base58_string(&inner))
                }
                None => None,
            },
            mnemonic: self.mnemonic.clone(),
            //Note: This is temporary
            connection: RpcClient::new(self.cluster_endpoint.to_string()),
            user_account: match &self.user_account {
                Some(kp) => {
                    let inner = kp.to_base58_string();
                    Some(Keypair::from_base58_string(&inner))
                }
                None => None,
            },
            network_identifier: self.network_identifier.clone(),
            cluster_endpoint: self.cluster_endpoint.clone(),
            public_keys: self.public_keys.clone(),
        }
    }
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
    pub fn initialize_random(&mut self) -> Result<()> {
        let account = SolanaWallet::create_random(PhraseType::Standard, None)?;
        self.payer_account = Some(account.get_keypair()?);
        self.mnemonic = Some(account.mnemonic.clone());
        self.user_account = Some(self.generate_user_keypair()?);
        self.accounts.push(account);
        Ok(())
    }

    pub fn initialize_from_mnemonic(&mut self, mnemonic: &str) -> Result<()> {
        let wallet = SolanaWallet::restore_from_mnemonic(None, mnemonic)?;
        self.initiralize_from_solana_wallet(&wallet)
    }

    pub fn initiralize_from_solana_wallet(&mut self, wallet: &SolanaWallet) -> Result<()> {
        let wallet = wallet.clone();
        self.payer_account = Keypair::from_bytes(&wallet.keypair).ok();
        self.mnemonic = Some(wallet.mnemonic.clone());
        self.accounts.push(wallet);
        Ok(())
    }

    pub fn get_payer_account(&self) -> Result<&Keypair> {
        self.payer_account
            .as_ref()
            .ok_or_else(|| anyhow!("Payer account unavailable"))
    }

    pub fn get_account_balance(&self) -> Result<u64> {
        ensure!(self.payer_account.is_some(), "Invalid payer account");
        let payer_account = self.payer_account.as_ref().unwrap();
        let commitment_config = CommitmentConfig::confirmed();
        let result = self
            .connection
            .get_balance_with_commitment(&payer_account.pubkey(), commitment_config)?;
        Ok(result.value)
    }

    pub fn request_air_drop(&self) -> Result<()> {
        let payer = self.get_payer_account()?;
        let payer_pubkey = payer.pubkey().to_string();
        let response = reqwest::blocking::Client::new()
            .post("https://faucet.satellite.one")
            .json(&warp_common::serde_json::json!({ "address": payer_pubkey }))
            .send()?
            .json::<ResponseStatus>()?;

        ensure!(response.status == "success", "Error requesting airdrop");
        Ok(())
    }

    pub fn request_air_drop_direct(&self, amount: u64) -> Result<()> {
        let sig = self
            .connection
            .request_airdrop(&self.get_payer_account()?.pubkey(), amount)?;
        self.connection
            .confirm_transaction_with_commitment(&sig, CommitmentConfig::confirmed())?;
        Ok(())
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

#[derive(
    warp_common::serde::Serialize, warp_common::serde::Deserialize, Debug, Clone, PartialEq, Eq,
)]
#[serde(crate = "warp_common::serde")]
struct ResponseStatus {
    pub status: String,
    #[serde(flatten)]
    pub additional: warp_common::serde_json::Value,
}

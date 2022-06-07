use anchor_client::solana_sdk::commitment_config::CommitmentConfig;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::{Client, Cluster};
use std::rc::Rc;

use crate::solana::wallet::SolanaWallet;

pub struct SolanaManager {
    pub wallet: SolanaWallet,
    pub connection: Client,
    pub cluster: Cluster,
}

impl std::fmt::Debug for SolanaManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaManager")
            .field("wallet", &self.wallet.get_pubkey())
            .field("connection", &"<>")
            .field("cluster", &self.cluster)
            .finish()
    }
}

impl SolanaManager {
    pub fn new(cluster: Cluster, wallet: &SolanaWallet) -> anyhow::Result<Self> {
        let connection = Client::new_with_options(
            cluster.clone(),
            Rc::new(wallet.get_keypair()?),
            CommitmentConfig::confirmed(),
        );
        let wallet = wallet.clone();
        Ok(Self {
            wallet,
            connection,
            cluster,
        })
    }

    pub fn get_account_balance(&self) -> anyhow::Result<u64> {
        let payer_account = self.wallet.get_pubkey()?;
        let commitment_config = CommitmentConfig::confirmed();
        let result = self
            .connection
            .program(Pubkey::new_unique())
            .rpc()
            .get_balance_with_commitment(&payer_account, commitment_config)?;
        Ok(result.value)
    }

    pub fn request_air_drop(&self) -> anyhow::Result<()> {
        let payer = self.wallet.get_pubkey()?.to_string();
        let fut = async {
            let response = reqwest::Client::new()
                .post("https://dev-faucet.satellite.one")
                .json(&serde_json::json!({ "address": payer, "accessCode": "blah"}))
                .send()
                .await?;

            if response.status().is_success() {
                let res = response.json::<ResponseStatus>().await?;
                if let Some(status) = res.status.as_ref() {
                    if status == "success" {
                        return Ok(());
                    }
                } else {
                    anyhow::bail!("{}", res.message);
                }
            } else {
                let res = response.json::<ResponseStatus>().await?;
                anyhow::bail!("{}", res.message);
            }
            Result::Ok::<(), anyhow::Error>(())
        };
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| handle.block_on(fut))
        } else {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .handle()
                .block_on(fut)
        }
    }

    pub fn request_air_drop_direct(&self, amount: u64) -> anyhow::Result<()> {
        let pubkey = self.wallet.get_pubkey()?;
        let connection = self.connection.program(Pubkey::new_unique()).rpc();

        let sig = connection.request_airdrop(&pubkey, amount)?;

        connection.confirm_transaction_with_commitment(&sig, CommitmentConfig::confirmed())?;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct ResponseStatus {
    pub status: Option<String>,
    pub message: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "transactionSignature"
    )]
    pub signature: Option<String>,
}

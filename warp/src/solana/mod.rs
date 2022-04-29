// Based on https://github.com/Satellite-im/Core-PWA/blob/dev/libraries/Solana/SolanaManager/SolanaManager.ts
// Note: This is a reference for `warp-mp-solana`.

pub mod helper;
pub mod manager;
pub mod wallet;

pub use anchor_client;
pub use bs58;

use anchor_client::solana_sdk::derivation_path::DerivationPath;
use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::keypair_from_seed_and_derivation_path;
use anchor_client::Cluster;
use anyhow::{anyhow, Result};
use derive_more::Display;

//TODO: Research and determine if solana supplies these URL internally
#[derive(Debug, Clone, Eq, PartialEq, Display)]
pub enum EndPoint {
    #[display(fmt = "https://api.mainnet-beta.solana.com")]
    MainNetBeta,
    #[display(fmt = "https://api.testnet.solana.com")]
    TestNet,
    #[display(fmt = "https://api.devnet.solana.com")]
    DevNet,
}

impl<A: AsRef<str>> From<A> for EndPoint {
    fn from(endpoint: A) -> Self {
        let endpoint = endpoint.as_ref();
        match endpoint {
            "main_net_beta" | "mainnetbeta" | "mainnet" => EndPoint::MainNetBeta,
            "testnet" | "test_net" => EndPoint::TestNet,
            _ => EndPoint::DevNet,
        }
    }
}

impl Into<Cluster> for EndPoint {
    fn into(self) -> Cluster {
        match self {
            EndPoint::MainNetBeta => todo!(),
            EndPoint::TestNet => todo!(),
            EndPoint::DevNet => todo!(),
        }
    }
}

pub fn derive_seed<U: AsRef<[u8]>>(seed: U) -> Result<Vec<u8>> {
    let der_path = DerivationPath::new_bip44(Some(0), Some(0));
    let keypair = keypair_from_seed_and_derivation_path(seed.as_ref(), Some(der_path))
        .map_err(|e| anyhow!(e.to_string()))?;
    Ok(keypair.to_bytes().to_vec())
}

/// Used to return a path for generating a new deterministic account.
pub fn get_path(index: u16) -> String {
    format!("m/44'/501'/{index}'/0'")
}

pub fn pubkey_from_seed<S: AsRef<str>>(
    seed_key: &Pubkey,
    seed: S,
    id: &Pubkey,
) -> Result<(Pubkey, Pubkey)> {
    pubkey_from_seeds(&[&seed_key.to_bytes()], seed, id)
}

pub fn pubkey_from_seeds<S: AsRef<str>>(
    seeds: &[&[u8]],
    seed: S,
    id: &Pubkey,
) -> Result<(Pubkey, Pubkey)> {
    let (base_pkey, _) = Pubkey::try_find_program_address(seeds, id)
        .ok_or_else(|| anyhow!("Error finding program"))?;
    let pkey = Pubkey::create_with_seed(&base_pkey, seed.as_ref(), id)?;
    Ok((base_pkey, pkey))
}

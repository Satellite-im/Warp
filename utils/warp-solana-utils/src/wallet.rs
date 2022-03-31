use solana_sdk::signature::{keypair_from_seed, Keypair};
use warp_common::anyhow::{anyhow, Result};
use warp_common::bip39::{Language, Mnemonic, MnemonicType, Seed};

#[derive(Clone)]
pub struct SolanaWallet {
    pub mnemonic: String,
    pub keypair: Vec<u8>,
    pub path: Option<String>,
    pub address: String,
}

impl std::fmt::Debug for SolanaWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaWallet")
            .field("mnemonic", &self.mnemonic)
            .field("keypair", &self.keypair)
            .field("path", &self.path)
            .field("address", &self.address)
            .finish()
    }
}

impl SolanaWallet {
    pub fn create_random_keypair() -> Result<SolanaWallet> {
        let mnemonic = Mnemonic::new(MnemonicType::Words12, Language::English);
        let seed = Seed::new(&mnemonic, "");
        let path = crate::get_path(0);
        let seed_with_path = crate::derive_seed(&seed, path.as_str())?;
        let keypair = keypair_from_seed(&seed_with_path).map_err(|e| anyhow!(e.to_string()))?;
        let address = keypair.to_base58_string();
        let mnemonic = mnemonic.into_phrase();
        Ok(SolanaWallet {
            mnemonic,
            keypair: seed_with_path,
            path: Some(path),
            address,
        })
    }

    pub fn restore_keypair_from_mnemonic(mnemonic: &str, index: u16) -> Result<SolanaWallet> {
        let mnemonic = Mnemonic::from_phrase(mnemonic, Language::English)?;
        let seed = Seed::new(&mnemonic, "");
        let path = crate::get_path(index);
        let seed_with_path = crate::derive_seed(&seed, path.as_str())?;
        let keypair = keypair_from_seed(&seed_with_path).map_err(|e| anyhow!(e.to_string()))?;
        let address = keypair.to_base58_string();
        let mnemonic = mnemonic.into_phrase();
        Ok(SolanaWallet {
            mnemonic,
            keypair: seed_with_path,
            path: Some(path),
            address,
        })
    }

    pub fn get_keypair(&self) -> Result<Keypair> {
        let keypair = keypair_from_seed(&self.keypair).map_err(|e| anyhow!(e.to_string()))?;
        Ok(keypair)
    }
}

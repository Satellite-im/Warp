use crate::get_path;
use anchor_client::solana_sdk::signature::{keypair_from_seed, Keypair, Signer};
use warp_common::anyhow::{anyhow, bail, Result};
use warp_common::bip39::{Language, Mnemonic, MnemonicType, Seed};
use warp_common::derive_more::Display;

pub struct SolanaWallet {
    pub mnemonic: String,
    pub keypair: [u8; 64],
}

impl Clone for SolanaWallet {
    fn clone(&self) -> Self {
        Self {
            mnemonic: self.mnemonic.clone(),
            keypair: self.keypair.clone(),
        }
    }
}

impl std::fmt::Debug for SolanaWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaWallet")
            .field("mnemonic", &self.mnemonic)
            .field("keypair", &self.keypair)
            .finish()
    }
}

#[derive(Clone, Display)]
pub enum PhraseType {
    #[display(fmt = "standard")]
    Standard,
    #[display(fmt = "secure")]
    Secure,
}

impl Default for PhraseType {
    fn default() -> Self {
        Self::Standard
    }
}

impl SolanaWallet {
    /// Generate a random keypair
    pub fn create_random(phrase: PhraseType, password: Option<&str>) -> Result<SolanaWallet> {
        let m_type = match phrase {
            PhraseType::Standard => MnemonicType::Words12,
            PhraseType::Secure => MnemonicType::Words24,
        };
        let mnemonic = Mnemonic::new(m_type, Language::English);
        Self::restore_from_mnemonic(password, mnemonic.phrase())
    }

    /// Restore keypair from a mnemonic phrase
    ///
    /// # Example
    ///
    /// ```
    /// use warp_solana_utils::wallet::SolanaWallet;
    ///
    /// let wallet = SolanaWallet::restore_from_mnemonic(None,
    ///         "morning caution dose lab six actress pond humble pause enact virtual train",
    /// ).unwrap();
    ///
    /// assert_eq!(wallet.address, String::from("68vtRPQcsV7ruWXa6Z8Enrb6TsXhbRzMywgCnEVyk7Va"))
    /// ```
    pub fn restore_from_mnemonic(password: Option<&str>, mnemonic: &str) -> Result<SolanaWallet> {
        let mnemonic = Mnemonic::from_phrase(mnemonic, Language::English)?;
        let seed = Seed::new(&mnemonic, password.unwrap_or_default());
        let seed_with_path = crate::derive_seed(&seed)?;
        let keypair = keypair_from_seed(&seed_with_path)
            .map_err(|e| anyhow!(e.to_string()))?
            .to_bytes();
        let mnemonic = mnemonic.into_phrase();
        Ok(SolanaWallet { mnemonic, keypair })
    }

    pub fn get_keypair(&self) -> Result<Keypair> {
        let kp = Keypair::from_bytes(&self.keypair)?;
        Ok(kp)
    }

    pub fn export_private_key(&self) -> Vec<u8> {
        unimplemented!()
    }

    pub fn test_mnemonic(&self, mnemonic: &str, password: Option<&str>) -> Result<()> {
        let mnemonic = Mnemonic::from_phrase(mnemonic, Language::English)?;
        let seed = Seed::new(&mnemonic, password.unwrap_or_default());
        let seed_with_path = crate::derive_seed(&seed)?;
        let keypair = keypair_from_seed(&seed_with_path)
            .map_err(|e| anyhow!(e.to_string()))?
            .to_bytes();
        if self.keypair != keypair {
            bail!("Mnemonic provided is not valid")
        }
        Ok(())
    }
}

use crate::Pubkey;
use anchor_client::solana_sdk::derivation_path::DerivationPath;
use anchor_client::solana_sdk::signature::{
    keypair_from_seed_and_derivation_path, Keypair, Signer,
};
use warp_common::anyhow::{anyhow, Result};
use warp_common::bip39::{Language, Mnemonic, MnemonicType, Seed};
use warp_common::derive_more::Display;
use warp_crypto::zeroize::Zeroize;

pub struct SolanaWallet {
    mnemonic: Vec<u8>,
    keypair: [u8; 64],
}

impl Clone for SolanaWallet {
    fn clone(&self) -> Self {
        Self {
            mnemonic: self.mnemonic.clone(),
            keypair: self.keypair,
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

impl Drop for SolanaWallet {
    fn drop(&mut self) {
        self.keypair.zeroize();
        self.mnemonic.zeroize();
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
    /// assert_eq!(wallet.get_pubkey().unwrap().to_string(), String::from("68vtRPQcsV7ruWXa6Z8Enrb6TsXhbRzMywgCnEVyk7Va"))
    /// ```
    pub fn restore_from_mnemonic(password: Option<&str>, mnemonic: &str) -> Result<SolanaWallet> {
        let mnemonic = Mnemonic::from_phrase(mnemonic, Language::English)?;
        let seed = Seed::new(&mnemonic, password.unwrap_or_default());

        let path = DerivationPath::new_bip44(Some(0), Some(0));
        let keypair = keypair_from_seed_and_derivation_path(seed.as_ref(), Some(path))
            .map_err(|e| anyhow!(e.to_string()))?
            .to_bytes();

        let mnemonic = mnemonic.entropy().to_vec();
        Ok(SolanaWallet { mnemonic, keypair })
    }

    pub fn get_keypair(&self) -> Result<Keypair> {
        let kp = Keypair::from_bytes(&self.keypair)?;
        Ok(kp)
    }

    pub fn get_pubkey(&self) -> Result<Pubkey> {
        let kp = self.get_keypair()?;
        Ok(kp.pubkey())
    }

    /// Obtains the mnemonic phrase
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
    /// assert_eq!(wallet.get_mnemonic_phrase().unwrap(), String::from("morning caution dose lab six actress pond humble pause enact virtual train"))
    /// ```
    pub fn get_mnemonic_phrase(&self) -> Result<String> {
        let phrase = Mnemonic::from_entropy(&self.mnemonic, Language::English)?;
        Ok(phrase.phrase().to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
use anyhow::anyhow;

#[cfg(not(target_arch = "wasm32"))]
use anchor_client::solana_sdk::{
    derivation_path::DerivationPath,
    pubkey::Pubkey,
    signature::{keypair_from_seed_and_derivation_path, Keypair, Signer},
};
use bip39::{Language, Mnemonic, MnemonicType, Seed};
use derive_more::Display;
#[cfg(target_arch = "wasm32")]
use solana_sdk::{
    derivation_path::DerivationPath,
    pubkey::Pubkey,
    signature::{keypair_from_seed_and_derivation_path, Keypair, Signer},
};
use wasm_bindgen::prelude::*;
use zeroize::Zeroize;

#[derive(Zeroize)]
#[wasm_bindgen]
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
            .field("mnemonic", &"<>")
            .field("keypair", &"<>")
            .finish()
    }
}

#[derive(Clone, Display, Copy)]
#[repr(C)]
#[wasm_bindgen]
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

#[cfg(not(target_arch = "wasm32"))]
impl SolanaWallet {
    /// Generate a random keypair
    pub fn create_random(
        phrase: PhraseType,
        password: Option<&str>,
    ) -> anyhow::Result<SolanaWallet> {
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
    ///
    /// use warp_mp_solana::solana::wallet::SolanaWallet;
    /// let wallet = SolanaWallet::restore_from_mnemonic(None,
    ///         "morning caution dose lab six actress pond humble pause enact virtual train",
    /// ).unwrap();
    ///
    /// assert_eq!(wallet.get_pubkey().unwrap().to_string(), String::from("68vtRPQcsV7ruWXa6Z8Enrb6TsXhbRzMywgCnEVyk7Va"))
    /// ```
    pub fn restore_from_mnemonic(
        password: Option<&str>,
        mnemonic: &str,
    ) -> anyhow::Result<SolanaWallet> {
        let mnemonic = Mnemonic::from_phrase(mnemonic, Language::English)?;
        let seed = Seed::new(&mnemonic, password.unwrap_or_default());

        let path = DerivationPath::new_bip44(Some(0), Some(0));
        let keypair = keypair_from_seed_and_derivation_path(seed.as_ref(), Some(path))
            .map_err(|e| anyhow!(e.to_string()))?
            .to_bytes();

        let mnemonic = mnemonic.entropy().to_vec();
        Ok(SolanaWallet { mnemonic, keypair })
    }

    pub fn get_keypair(&self) -> anyhow::Result<Keypair> {
        let kp = Keypair::from_bytes(&self.keypair)?;
        Ok(kp)
    }

    pub fn get_pubkey(&self) -> anyhow::Result<Pubkey> {
        let kp = self.get_keypair()?;
        Ok(kp.pubkey())
    }

    /// Obtains the mnemonic phrase
    ///
    /// # Example
    ///
    /// ```
    ///
    /// use warp_mp_solana::solana::wallet::SolanaWallet;
    /// let wallet = SolanaWallet::restore_from_mnemonic(None,
    ///         "morning caution dose lab six actress pond humble pause enact virtual train",
    /// ).unwrap();
    ///
    /// assert_eq!(wallet.get_mnemonic_phrase().unwrap(), String::from("morning caution dose lab six actress pond humble pause enact virtual train"))
    /// ```
    pub fn get_mnemonic_phrase(&self) -> anyhow::Result<String> {
        let phrase = Mnemonic::from_entropy(&self.mnemonic, Language::English)?;
        Ok(phrase.phrase().to_string())
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl SolanaWallet {
    /// Generate a random keypair
    #[wasm_bindgen]
    pub fn create_random(
        phrase: PhraseType,
        password: Option<String>,
    ) -> std::result::Result<SolanaWallet, JsError> {
        let m_type = match phrase {
            PhraseType::Standard => MnemonicType::Words12,
            PhraseType::Secure => MnemonicType::Words24,
        };
        let mnemonic = Mnemonic::new(m_type, Language::English);
        Self::restore_from_mnemonic(password, mnemonic.phrase())
    }

    /// Restore keypair from a mnemonic phrase
    #[wasm_bindgen]
    pub fn restore_from_mnemonic(
        password: Option<String>,
        mnemonic: &str,
    ) -> std::result::Result<SolanaWallet, JsError> {
        let mnemonic = Mnemonic::from_phrase(mnemonic, Language::English)
            .map_err(|e| JsError::new(&e.to_string()))?;
        let seed = Seed::new(&mnemonic, password.as_deref().unwrap_or_default());

        let path = DerivationPath::new_bip44(Some(0), Some(0));
        let keypair = keypair_from_seed_and_derivation_path(seed.as_ref(), Some(path))
            .map_err(|_| JsError::new("Unable to derive keypair from path"))?
            .to_bytes();

        let mnemonic = mnemonic.entropy().to_vec();
        Ok(SolanaWallet { mnemonic, keypair })
    }

    #[wasm_bindgen]
    pub fn get_keypair(&self) -> std::result::Result<Keypair, JsError> {
        let kp = Keypair::from_bytes(&self.keypair).map_err(|e| JsError::from(e))?;
        Ok(kp)
    }

    #[wasm_bindgen]
    pub fn get_pubkey(&self) -> std::result::Result<Pubkey, JsError> {
        let kp = self.get_keypair().map_err(|e| JsError::from(e))?;
        Ok(kp.pubkey())
    }

    /// Obtains the mnemonic phrase
    #[wasm_bindgen]
    pub fn get_mnemonic_phrase(&self) -> std::result::Result<String, JsError> {
        let phrase = Mnemonic::from_entropy(&self.mnemonic, Language::English)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(phrase.phrase().to_string())
    }
}

#![allow(clippy::result_large_err)]
use crate::{error::Error, tesseract::Tesseract};
use bip39::{Language, Mnemonic, MnemonicType, Seed};
use derive_more::Display;
use did_key::KeyMaterial;
use ed25519_dalek::{Keypair, PublicKey, SecretKey, KEYPAIR_LENGTH, SECRET_KEY_LENGTH};
use hmac::{Hmac, Mac};
use sha2::Sha512;
use zeroize::Zeroizing;

use super::DID;

const ED25519_BIP32_NAME: &str = "ed25519 seed";
type HmacSha512 = Hmac<Sha512>;

#[derive(Clone, Display, Copy)]
#[repr(C)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
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

pub fn generate_mnemonic_phrase(phrase: PhraseType) -> Mnemonic {
    let m_type = match phrase {
        PhraseType::Standard => MnemonicType::Words12,
        PhraseType::Secure => MnemonicType::Words24,
    };
    Mnemonic::new(m_type, Language::English)
}

pub fn generate_keypair(
    phrase: PhraseType,
    passphrase: Option<&str>,
) -> Result<(String, DID), Error> {
    let mnemonic = generate_mnemonic_phrase(phrase);
    let did = did_from_mnemonic(mnemonic.phrase(), passphrase)?;
    Ok((mnemonic.into_phrase(), did))
}

/// Generate DID from mnemonic phrase, extending compatibility
pub fn did_from_mnemonic_with_chain(
    mnemonic: &str,
    passphrase: Option<&str>,
) -> Result<(DID, [u8; 32]), Error> {
    let mnemonic = Mnemonic::from_phrase(mnemonic, Language::English)?;
    let seed = Seed::new(&mnemonic, passphrase.unwrap_or_default());
    let mut mac = HmacSha512::new_from_slice(ED25519_BIP32_NAME.as_ref()).unwrap();
    mac.update(seed.as_bytes());
    let bytes = mac.finalize().into_bytes();
    let secret = SecretKey::from_bytes(&bytes[..32])?;
    // Note: This will allow extending to `ed25519-dalek-bip32` for path derivation, with this being the root of the `ExtendedSecretKey` in the following format
    /*
       ExtendedSecretKey {
           depth: 0,
           child_index: ChildIndex::Normal(0),
           secret_key,
           chain_code
       }
    */
    let mut chain_code = [0; 32];
    chain_code.copy_from_slice(&bytes[32..]);
    Ok((secret.into(), chain_code))
}

/// Generate DID from mnemonic phrase
pub fn did_from_mnemonic(mnemonic: &str, passphrase: Option<&str>) -> Result<DID, Error> {
    did_from_mnemonic_with_chain(mnemonic, passphrase).map(|(did, _)| did)
}

pub fn mnemonic_into_tesseract(
    tesseract: &Tesseract,
    mnemonic: &str,
    passphrase: Option<&str>,
    save_mnemonic: bool,
) -> Result<(), Error> {
    if !tesseract.is_unlock() {
        return Err(Error::TesseractLocked);
    }

    if tesseract.exist("keypair") {
        return Err(Error::Any(anyhow::anyhow!("Keypair already exist")));
    }

    let (did, chain) = did_from_mnemonic_with_chain(mnemonic, passphrase)?;

    let bytes = Zeroizing::new(did.as_ref().private_key_bytes());
    let secret_key = SecretKey::from_bytes(&bytes)?;
    let public_key: PublicKey = (&secret_key).into();
    let mut bytes: Zeroizing<[u8; KEYPAIR_LENGTH]> = Zeroizing::new([0u8; KEYPAIR_LENGTH]);

    bytes[..SECRET_KEY_LENGTH].copy_from_slice(secret_key.as_bytes());
    bytes[SECRET_KEY_LENGTH..].copy_from_slice(public_key.as_bytes());

    let kp = Keypair::from_bytes(&*bytes)?;

    let encoded = Zeroizing::new(bs58::encode(&kp.to_bytes()).into_string());

    tesseract.set("keypair", &encoded)?;

    if save_mnemonic {
        let encoded_chain = Zeroizing::new(bs58::encode(&chain).into_string());
        tesseract.set("chain", &encoded_chain)?;
        tesseract.set("mnemonic", mnemonic)?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use std::{
        ffi::{CStr, CString},
        os::raw::c_char,
    };

    use crate::{
        error::Error,
        ffi::{FFIResult, FFIResult_Null},
        tesseract::Tesseract,
    };

    use super::{PhraseType, DID};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn generate_mnemonic_phrase(phrase_type: PhraseType) -> *mut c_char {
        let mnemonic = super::generate_mnemonic_phrase(phrase_type);
        match CString::new(mnemonic.into_phrase()) {
            Ok(cstr) => cstr.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn did_from_mnemonic(phrase: *const c_char) -> FFIResult<DID> {
        if phrase.is_null() {
            return FFIResult::err(Error::from(anyhow::anyhow!("phrase is null")));
        }

        let phrase = CStr::from_ptr(phrase).to_string_lossy().to_string();
        super::did_from_mnemonic(&phrase, None).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mnemonic_into_tesseract(
        tesseract: *const Tesseract,
        phrase: *const c_char,
        save: bool,
    ) -> FFIResult_Null {
        if tesseract.is_null() {
            return FFIResult_Null::err(Error::from(anyhow::anyhow!("Tesseract cannot be null")));
        }

        if phrase.is_null() {
            return FFIResult_Null::err(Error::from(anyhow::anyhow!("Phrase cannot be null")));
        }

        let phrase = CStr::from_ptr(phrase).to_string_lossy().to_string();

        super::mnemonic_into_tesseract(&*tesseract, &phrase, None, save).into()
    }
}

#[cfg(test)]
mod test {
    use super::did_from_mnemonic;

    const PHRASE: &str =
        "morning caution dose lab six actress pond humble pause enact virtual train";

    #[test]
    fn generate_did_from_phrase() -> anyhow::Result<()> {
        let expected = "did:key:z6MksiU5wFcZHHSp4VvtQePW4zwUDNmGADqxfQi4TdcEvmjz";
        let did = did_from_mnemonic(PHRASE, None)?;
        assert_eq!(did.to_string(), expected);
        Ok(())
    }
}

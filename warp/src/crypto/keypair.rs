use crate::error::Error;
use bip39::{Language, Mnemonic, MnemonicType, Seed};
use derive_more::Display;
use ed25519_dalek::SecretKey;
use hmac::{Hmac, Mac};
use sha2::Sha512;

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

pub fn generate_keypair(phrase: PhraseType, passphrase: Option<&str>) -> Result<(String, DID), Error> {
    let mnemonic = generate_mnemonic_phrase(phrase);
    let did = did_from_mnemonic(mnemonic.phrase(), passphrase)?;
    Ok((mnemonic.into_phrase(), did))
}

pub fn did_from_mnemonic(mnemonic: &str, passphrase: Option<&str>) -> Result<DID, Error> {
    let mnemonic = Mnemonic::from_phrase(mnemonic, Language::English)?;
    let seed = Seed::new(&mnemonic, passphrase.unwrap_or_default());
    let mut mac = HmacSha512::new_from_slice(ED25519_BIP32_NAME.as_ref()).unwrap();
    mac.update(seed.as_bytes());
    let bytes = mac.finalize().into_bytes();
    let secret = SecretKey::from_bytes(&bytes[..32])?;
    Ok(secret.into())
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use std::{
        ffi::{CStr, CString},
        os::raw::c_char,
    };

    use crate::{error::Error, ffi::FFIResult};

    use super::{DID, PhraseType};

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
}


#[cfg(test)]
mod test {
    use super::did_from_mnemonic;

    const PHRASE: &str = "morning caution dose lab six actress pond humble pause enact virtual train";

    #[test]
    fn generate_did_from_phrase() -> anyhow::Result<()> {
        let expected = "did:key:z6MksiU5wFcZHHSp4VvtQePW4zwUDNmGADqxfQi4TdcEvmjz";
        let did = did_from_mnemonic(PHRASE, None)?;
        assert_eq!(did.to_string(), expected);
        Ok(())
    }
}

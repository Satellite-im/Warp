use std::collections::HashMap;
use warp_crypto::zeroize::Zeroize;

use wasm_bindgen::prelude::*;

/// The key store that holds encrypted strings that can be used for later use.
#[derive(Default, Clone, PartialEq, Eq)]
#[wasm_bindgen]
pub struct Tesseract {
    internal: HashMap<String, Vec<u8>>,
    enc_pass: Vec<u8>,
    unlock: bool,
}

impl Drop for Tesseract {
    fn drop(&mut self) {
        if self.is_unlock() {
            self.lock()
        }
    }
}

#[wasm_bindgen]
impl Tesseract {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Tesseract {
        Tesseract::default()
    }

    /// Import and encrypt a hashmap into tesseract
    #[wasm_bindgen]
    pub fn import(passphrase: &[u8], map: JsValue) -> Result<Tesseract, JsError> {
        let map: HashMap<String, String> = serde_wasm_bindgen::from_value(map).unwrap();
        let mut tesseract = Tesseract::default();
        tesseract.unlock(passphrase)?;
        for (key, val) in map {
            tesseract.set(key.as_str(), val.as_str())?;
        }
        Ok(tesseract)
    }

    /// To store a value to be encrypted into the keystore. If the key already exist, it
    /// will be overwritten.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), JsError> {
        if !self.is_unlock() {
            return Err(JsError::new("Data is secured."));
        }
        let pkey = warp_crypto::cipher::aes256gcm_self_decrypt(&self.enc_pass)?;
        let data = warp_crypto::cipher::aes256gcm_encrypt(&pkey, value.as_bytes())?;
        self.internal.insert(key.to_string(), data);
        Ok(())
    }

    /// Check to see if the key store contains the key
    pub fn exist(&self, key: &str) -> bool {
        self.internal.contains_key(key)
    }

    /// Used to retreive and decrypt the value stored for the key
    pub fn retrieve(&self, key: &str) -> Result<String, JsError> {
        if !self.is_unlock() {
            return Err(JsError::new("Data is secured."));
        }

        if !self.exist(key) {
            return Err(JsError::new("Key is not found"));
        }

        let pkey = warp_crypto::cipher::aes256gcm_self_decrypt(&self.enc_pass)?;

        let data = self.internal.get(key).unwrap();
        let slice = warp_crypto::cipher::aes256gcm_decrypt(&pkey, data)?;
        let plain_text = String::from_utf8_lossy(&slice[..]).to_string();
        Ok(plain_text)
    }

    /// Used to delete the value from the keystore
    pub fn delete(&mut self, key: &str) -> Result<(), JsError> {
        self.internal
            .remove(key)
            .ok_or_else(|| JsError::new("Could not remove key. Item does not exist"))?;
        Ok(())
    }

    /// Used to clear the whole keystore.
    pub fn clear(&mut self) {
        self.internal.clear();
    }

    /// Decrypts and export tesseract contents to a `HashMap`
    pub fn export(&self) -> Result<JsValue, JsError> {
        if !self.is_unlock() {
            return Err(JsError::new("Data is secured."));
        }
        let mut map = HashMap::new();
        for key in self.internal.keys() {
            let value = match self.retrieve(key) {
                Ok(v) => v,
                Err(_) => continue,
            };
            map.insert(key.clone(), value);
        }
        Ok(serde_wasm_bindgen::to_value(&map).unwrap())
    }

    /// Checks to see if tesseract is secured and not "unlocked"
    pub fn is_unlock(&self) -> bool {
        !self.enc_pass.is_empty() && self.unlock
    }

    /// Decrypts and store the password and plaintext contents in memory
    pub fn unlock(&mut self, passphrase: &[u8]) -> Result<(), JsError> {
        self.enc_pass = warp_crypto::cipher::aes256gcm_self_encrypt(passphrase)?;
        self.unlock = true;
        Ok(())
    }

    /// Encrypts and remove password and plaintext from memory.
    ///
    /// Note: This will override existing contents within Tesseract.
    pub fn lock(&mut self) {
        self.enc_pass.zeroize();
        self.unlock = false;
    }
}

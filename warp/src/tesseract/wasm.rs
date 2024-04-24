use crate::tesseract::{self, TesseractEvent};
use wasm_bindgen::{prelude::*};
use futures::{stream::BoxStream, StreamExt};

#[wasm_bindgen]
#[derive(Default)]
pub struct Tesseract {
    inner: tesseract::Tesseract,
}

#[wasm_bindgen]
impl Tesseract {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Tesseract {
        Tesseract::default()
    }
    
    pub fn set_autosave(&self) {
        self.inner.set_autosave();
    }

    pub fn autosave_enabled(&self) -> bool {
        self.inner.autosave_enabled()
    }

    pub fn disable_key_check(&self) {
        self.inner.disable_key_check();
    }

    pub fn enable_key_check(&self) {
        self.inner.enable_key_check();
    }

    pub fn is_key_check_enabled(&self) -> bool {
        self.inner.is_key_check_enabled()
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), JsError> {
        self.inner.set(key, value).map_err(|e| e.into())
    }

    pub fn exist(&self, key: &str) -> bool {
        self.inner.exist(key)
    }

    pub fn retrieve(&self, key: &str) -> Result<String, JsError> {
        self.inner.retrieve(key).map_err(|e| e.into())
    }

    pub fn update_unlock(&self, old_passphrase: &[u8], new_passphrase: &[u8]) -> Result<(), JsError> {
        self.inner.update_unlock(old_passphrase, new_passphrase).map_err(|e| e.into())
    }

    pub fn delete(&self, key: &str) -> Result<(), JsError> {
        self.inner.delete(key).map_err(|e| e.into())
    }

    pub fn clear(&self) {
        self.inner.clear();
    }

    pub fn is_unlock(&self) -> bool {
        self.inner.is_unlock()
    }

    pub fn unlock(&self, passphrase: &[u8]) -> Result<(), JsError> {
        self.inner.unlock(passphrase).map_err(|e| e.into())
    }

    pub fn lock(&self) {
        self.inner.lock();
    }
    
    pub fn save(&self) -> Result<(), JsError> {
        self.inner.save().map_err(|e| e.into())
    }
    
    // pub async fn subscribe(&self) -> JsValue {
    //     self.inner.subscribe();
    // }

    pub fn load_from_storage(&self) -> Result<(), JsError> {
        self.inner.load_from_storage().map_err(|e| e.into())
    }
}

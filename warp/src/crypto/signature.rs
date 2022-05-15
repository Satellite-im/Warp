use crate::error::Error;
use ed25519_dalek::{Keypair, PublicKey, Signature, Signer};
use rand::rngs::OsRng;
use wasm_bindgen::prelude::wasm_bindgen;
use zeroize::Zeroize;

/// An Ed25519 Keypair Helper
#[derive(Debug)]
#[wasm_bindgen]
pub struct Ed25519Keypair(Keypair);

#[derive(Copy, Clone, Eq, PartialEq)]
#[wasm_bindgen]
pub struct Ed25519PublicKey(PublicKey);

impl Zeroize for Ed25519Keypair {
    fn zeroize(&mut self) {
        self.0.secret.zeroize()
    }
}

impl Drop for Ed25519Keypair {
    fn drop(&mut self) {
        self.zeroize()
    }
}

#[wasm_bindgen]
impl Ed25519Keypair {
    /// Creates a new keypair
    #[wasm_bindgen(constructor)]
    pub fn new() -> Ed25519Keypair {
        let mut csprng = OsRng {};
        Ed25519Keypair(Keypair::generate(&mut csprng))
    }

    /// Import keypair from 64 bytes
    #[wasm_bindgen]
    pub fn from_bytes(bytes: &[u8]) -> Result<Ed25519Keypair, Error> {
        Keypair::from_bytes(bytes)
            .map(Ed25519Keypair)
            .map_err(Error::from)
    }

    /// Public key from ed25519 keypair
    #[wasm_bindgen]
    pub fn public_key(&self) -> Ed25519PublicKey {
        Ed25519PublicKey(self.0.public)
    }

    /// Sign a message with the keypair
    #[wasm_bindgen]
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        let sig = self.0.sign(data);
        sig.to_bytes().to_vec()
    }

    /// Verify a signature with the keypair
    #[wasm_bindgen]
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<(), Error> {
        let signature = Signature::from_bytes(signature)?;
        self.0.verify(data, &signature).map_err(Error::from)
    }
}

impl Ed25519Keypair {
    pub fn to_inner(&self) -> Result<Keypair, Error> {
        Keypair::from_bytes(&self.0.to_bytes()).map_err(Error::from)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Ed25519Keypair {
    /// Sign with a keypair from the reader
    pub fn sign_reader(&self, reader: &mut impl std::io::Read) -> Result<Signature, Error> {
        use curve25519_dalek::digest::Digest;
        use ed25519_dalek::Sha512;

        let mut digest: Sha512 = Sha512::new();
        std::io::copy(reader, &mut digest)?;
        self.0.sign_prehashed(digest, None).map_err(Error::from)
    }

    /// Verify a signature using the keypair and reader
    pub fn verify_reader(
        &self,
        reader: &mut impl std::io::Read,
        signature: &[u8],
    ) -> Result<(), Error> {
        use curve25519_dalek::digest::Digest;
        use ed25519_dalek::Sha512;

        let signature = Signature::from_bytes(signature)?;
        let mut digest: Sha512 = Sha512::new();
        std::io::copy(reader, &mut digest)?;
        self.0
            .verify_prehashed(digest, None, &signature)
            .map_err(Error::from)
    }
}

use crate::crypto::signature::{Ed25519Keypair, Ed25519PublicKey};
use crate::error::Error;
use curve25519_dalek::edwards::CompressedEdwardsY;
use ed25519_dalek::{Digest, Sha512};
use warp_derive::FFIFree;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(FFIFree)]
pub struct X25519Secret(StaticSecret);

#[derive(Debug, Copy, Clone, Eq, PartialEq, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct X25519PublicKey(PublicKey);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl X25519PublicKey {
    /// Import X25519 Public Key from bytes
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_bytes(bytes: &[u8]) -> X25519PublicKey {
        let mut public_bytes = [0u8; 32];
        public_bytes.copy_from_slice(bytes);
        X25519PublicKey(PublicKey::from(public_bytes))
    }

    /// Convert a ED25519 Public Key to X25519 Public Key
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_ed25519_public_key(public_key: Ed25519PublicKey) -> Result<X25519PublicKey, Error> {
        let public_key = public_key.to_inner();
        let decompressed_point = CompressedEdwardsY(public_key.to_bytes())
            .decompress()
            .ok_or(Error::Other)?;
        let mon = decompressed_point.to_montgomery();
        Ok(X25519PublicKey(PublicKey::from(mon.0)))
    }
}

impl TryFrom<Ed25519PublicKey> for X25519PublicKey {
    type Error = Error;
    fn try_from(value: Ed25519PublicKey) -> Result<Self, Self::Error> {
        X25519PublicKey::from_ed25519_public_key(value)
    }
}

impl X25519PublicKey {
    /// To access X25519 Public Key
    pub fn to_inner(&self) -> PublicKey {
        self.0
    }
}

impl Default for X25519Secret {
    fn default() -> Self {
        X25519Secret::new()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl X25519Secret {
    /// Create an X25519 Secret Key
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> X25519Secret {
        let mut key = [0u8; 32];
        //Note: This is being unwrapped only to assume that it will not error out
        getrandom::getrandom(&mut key).unwrap();
        X25519Secret::from_bytes(&key)
    }

    /// Import X25519 Secret Key from bytes
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_bytes(bytes: &[u8]) -> X25519Secret {
        let mut secret_bytes = [0u8; 32];
        secret_bytes.copy_from_slice(bytes);
        X25519Secret(x25519_dalek::StaticSecret::from(secret_bytes))
    }

    /// Convert ED25519 to X25519
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_ed25519_keypair(keypair: &Ed25519Keypair) -> Result<X25519Secret, Error> {
        let ed25519_keypair = keypair.to_inner()?;
        let mut hasher: Sha512 = Sha512::new();
        hasher.update(ed25519_keypair.secret.as_ref());
        let hash = hasher.finalize().to_vec();
        let mut new_sk: [u8; 32] = [0; 32];
        new_sk.copy_from_slice(&hash[..32]);
        let sk = x25519_dalek::StaticSecret::from(new_sk);
        new_sk.zeroize();
        Ok(X25519Secret(sk))
    }

    /// Public Key of X25519
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn public_key(&self) -> X25519PublicKey {
        X25519PublicKey(PublicKey::from(&self.0))
    }

    /// Key exchange
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn key_exchange(&self, public_key: X25519PublicKey) -> Vec<u8> {
        self.0
            .diffie_hellman(&public_key.to_inner())
            .as_bytes()
            .to_vec()
    }
}

impl TryFrom<Ed25519Keypair> for X25519Secret {
    type Error = Error;
    fn try_from(value: Ed25519Keypair) -> Result<Self, Self::Error> {
        X25519Secret::from_ed25519_keypair(&value)
    }
}

#[cfg(test)]
mod test {
    use crate::crypto::cipher::*;
    use crate::crypto::exchange::*;

    #[test]
    fn key_exchange() -> anyhow::Result<()> {
        let alice_private_key = X25519Secret::new();
        let alice_public_key = alice_private_key.public_key();

        let bob_private_key = X25519Secret::new();
        let bob_public_key = bob_private_key.public_key();

        let a_dh = alice_private_key.key_exchange(bob_public_key);
        let b_dh = bob_private_key.key_exchange(alice_public_key);

        assert_eq!(a_dh, b_dh);

        Ok(())
    }

    #[test]
    fn ed25519_key_exchange() -> anyhow::Result<()> {
        let alice_keypair = Ed25519Keypair::new()?;
        let bob_keypair = Ed25519Keypair::new()?;

        let alice_secret = X25519Secret::from_ed25519_keypair(&alice_keypair)?;
        let alice_pubkey = alice_secret.public_key();

        let bob_secret = X25519Secret::from_ed25519_keypair(&bob_keypair)?;
        let bob_pubkey = bob_secret.public_key();

        let a_dh = alice_secret.key_exchange(bob_pubkey);
        let b_dh = bob_secret.key_exchange(alice_pubkey);

        assert_eq!(a_dh, b_dh);

        Ok(())
    }

    #[test]
    fn ed25519_pk_to_x25519_pk() -> anyhow::Result<()> {
        let keypair = Ed25519Keypair::new()?;
        let ed25519_pk = keypair.public_key();

        let x25519_kp = X25519Secret::from_ed25519_keypair(&keypair)?;
        let x25519_pk = X25519PublicKey::from_ed25519_public_key(ed25519_pk)?;

        assert_eq!(x25519_kp.public_key(), x25519_pk);

        Ok(())
    }

    #[test]
    fn ed25519_key_exchange_encryption() -> anyhow::Result<()> {
        let alice_keypair = Ed25519Keypair::new()?;
        let bob_keypair = Ed25519Keypair::new()?;

        let alice_secret = X25519Secret::from_ed25519_keypair(&alice_keypair)?;
        let alice_pubkey = alice_secret.public_key();

        let bob_secret = X25519Secret::from_ed25519_keypair(&bob_keypair)?;
        let bob_pubkey = bob_secret.public_key();

        let a_dh = alice_secret.key_exchange(bob_pubkey);
        let b_dh = bob_secret.key_exchange(alice_pubkey);

        assert_eq!(a_dh, b_dh);

        {
            let cipher = Cipher::from(&a_dh);
            let for_bob = cipher.encrypt(CipherType::Aes256Gcm, b"Hello Bob")?;
            let plaintext = cipher
                .decrypt(CipherType::Aes256Gcm, &for_bob)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Bob"));
        }

        {
            let cipher = Cipher::from(&b_dh);
            let for_alice = cipher.encrypt(CipherType::Aes256Gcm, b"Hello Alice")?;
            let plaintext = cipher
                .decrypt(CipherType::Aes256Gcm, &for_alice)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Alice"));
        }

        Ok(())
    }

    #[test]
    fn ed25519_key_exchange_encryption_using_ed25519_pk() -> anyhow::Result<()> {
        // Used to test the public key conversion.
        let alice_keypair = Ed25519Keypair::new()?;
        let bob_keypair = Ed25519Keypair::new()?;

        let alice_secret = X25519Secret::from_ed25519_keypair(&alice_keypair)?;
        let alice_pubkey = X25519PublicKey::from_ed25519_public_key(alice_keypair.public_key())?;

        let bob_secret = X25519Secret::from_ed25519_keypair(&bob_keypair)?;
        let bob_pubkey = X25519PublicKey::from_ed25519_public_key(bob_keypair.public_key())?;

        let a_dh = alice_secret.key_exchange(bob_pubkey);
        let b_dh = bob_secret.key_exchange(alice_pubkey);

        assert_eq!(a_dh, b_dh);

        {
            let cipher = Cipher::from(&a_dh);
            let for_bob = cipher.encrypt(CipherType::Aes256Gcm, b"Hello Bob")?;
            let plaintext = cipher
                .decrypt(CipherType::Aes256Gcm, &for_bob)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Bob"));
        }

        {
            let cipher = Cipher::from(&b_dh);
            let for_alice = cipher.encrypt(CipherType::Aes256Gcm, b"Hello Alice")?;
            let plaintext = cipher
                .decrypt(CipherType::Aes256Gcm, &for_alice)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Alice"));
        }

        Ok(())
    }

    #[test]
    fn key_exchange_encryption() -> anyhow::Result<()> {
        let alice_private_key = X25519Secret::new();
        let alice_public_key = alice_private_key.public_key();

        let bob_private_key = X25519Secret::new();
        let bob_public_key = bob_private_key.public_key();

        let a_dh = alice_private_key.key_exchange(bob_public_key);
        let b_dh = bob_private_key.key_exchange(alice_public_key);

        assert_eq!(a_dh, b_dh);

        {
            let cipher = Cipher::from(&a_dh);
            let for_bob = cipher.encrypt(CipherType::Aes256Gcm, b"Hello Bob")?;
            let plaintext = cipher
                .decrypt(CipherType::Aes256Gcm, &for_bob)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Bob"));
        }

        {
            let cipher = Cipher::from(&b_dh);
            let for_alice = cipher.encrypt(CipherType::Aes256Gcm, b"Hello Alice")?;
            let plaintext = cipher
                .decrypt(CipherType::Aes256Gcm, &for_alice)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Alice"));
        }

        Ok(())
    }
}

use crate::crypto::signature::Ed25519Keypair;
use crate::error::Error;
use warp_derive::FFIFree;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use x25519_dalek::{PublicKey, StaticSecret};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(FFIFree)]
pub struct X25519Secret(StaticSecret);

#[derive(Copy, Clone, Eq, PartialEq, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct X25519PublicKey(PublicKey);

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl X25519PublicKey {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_bytes(bytes: &[u8]) -> X25519PublicKey {
        let mut public_bytes = [0u8; 32];
        public_bytes.copy_from_slice(bytes);
        X25519PublicKey(PublicKey::from(public_bytes))
    }
}

impl X25519PublicKey {
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
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> X25519Secret {
        let mut key = [0u8; 32];
        //Note: This is being unwrapped only to assume that it will not error out
        getrandom::getrandom(&mut key).unwrap();
        X25519Secret::from_bytes(&key)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_bytes(bytes: &[u8]) -> X25519Secret {
        let mut secret_bytes = [0u8; 32];
        secret_bytes.copy_from_slice(bytes);
        X25519Secret(x25519_dalek::StaticSecret::from(secret_bytes))
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn from_ed25519_keypair(keypair: &Ed25519Keypair) -> Result<X25519Secret, Error> {
        let ed25519_keypair = keypair.to_inner()?;
        Ok(X25519Secret(StaticSecret::from(
            ed25519_keypair.secret.to_bytes(),
        )))
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn public_key(&self) -> X25519PublicKey {
        X25519PublicKey(PublicKey::from(&self.0))
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn key_exchange(&self, public_key: X25519PublicKey) -> Vec<u8> {
        self.0
            .diffie_hellman(&public_key.to_inner())
            .as_bytes()
            .to_vec()
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
    fn loop_key_exchange() -> anyhow::Result<()> {
        for _ in 0..50 {
            let alice_private_key = X25519Secret::new();
            let alice_public_key = alice_private_key.public_key();

            let bob_private_key = X25519Secret::new();
            let bob_public_key = bob_private_key.public_key();

            let a_dh = alice_private_key.key_exchange(bob_public_key);
            let b_dh = bob_private_key.key_exchange(alice_public_key);

            assert_eq!(a_dh, b_dh);
        }

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
            let for_bob = aes256gcm_encrypt(&a_dh, &b"Hello Bob"[..])?;
            let plaintext = aes256gcm_decrypt(&b_dh, &for_bob)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Bob"));
        }

        {
            let for_alice = aes256gcm_encrypt(&b_dh, &b"Hello Alice"[..])?;
            let plaintext = aes256gcm_decrypt(&a_dh, &for_alice)
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
            let for_bob = aes256gcm_encrypt(&a_dh, &b"Hello Bob"[..])?;
            let plaintext = aes256gcm_decrypt(&b_dh, &for_bob)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Bob"));
        }

        {
            let for_alice = aes256gcm_encrypt(&b_dh, &b"Hello Alice"[..])?;
            let plaintext = aes256gcm_decrypt(&a_dh, &for_alice)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Alice"));
        }

        Ok(())
    }
}

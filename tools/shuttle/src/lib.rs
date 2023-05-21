pub mod agent;
pub mod client;
pub mod payload;
pub mod request;
pub mod response;
pub mod server;
pub mod store;

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use curve25519_dalek::edwards::CompressedEdwardsY;
use libipld::Multihash;
use rand::RngCore;
use rust_ipfs::{Keypair, PublicKey, PeerId};
use sha2::{Digest, Sha256, Sha512};
use x25519_dalek::StaticSecret;

// Max transmission size
pub(crate) const MAX_TRANSMIT_SIZE: usize = 8 * 1024 * 1024;

pub(crate) fn encrypt(data: &[u8], key: &[u8]) -> anyhow::Result<Vec<u8>> {
    let nonce = generate_nonce();

    let key = zeroize::Zeroizing::new(sha256_hash(key, &nonce));

    let cipher = Aes256Gcm::new(key.as_slice().into());
    let mut cipher_data = cipher
        .encrypt(nonce.as_slice().into(), data)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    cipher_data.extend(nonce);

    Ok(cipher_data)
}

/// Used to decrypt data
pub(crate) fn decrypt(data: &[u8], key: &[u8]) -> anyhow::Result<Vec<u8>> {
    let (nonce, payload) = extract_data_slice(data, 12);

    let key = zeroize::Zeroizing::new(sha256_hash(key, nonce));

    let cipher = Aes256Gcm::new(key.as_slice().into());
    cipher
        .decrypt(nonce.into(), payload)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

fn sha256_hash(data: &[u8], salt: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.update(salt);
    hasher.finalize().to_vec()
}

fn sha256_iter(iter: impl Iterator<Item = Option<impl AsRef<[u8]>>>) -> Vec<u8> {
    let mut hasher = Sha256::new();
    for data in iter.flatten() {
        hasher.update(data);
    }
    hasher.finalize().to_vec()
}

fn generate_nonce() -> Vec<u8> {
    let mut buf = vec![0u8; 12];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut buf);
    buf
}

fn extract_data_slice(data: &[u8], size: usize) -> (&[u8], &[u8]) {
    let extracted = &data[data.len() - size..];
    let payload = &data[..data.len() - size];
    (extracted, payload)
}

fn convert_ed25519_to_x25519_priv(
    secret: &rust_ipfs::libp2p::identity::ed25519::SecretKey,
) -> StaticSecret {
    let mut hasher: Sha512 = Sha512::new();
    hasher.update(secret);
    let hash = hasher.finalize().to_vec();
    let mut new_sk: [u8; 32] = [0; 32];
    new_sk.copy_from_slice(&hash[..32]);
    x25519_dalek::StaticSecret::from(new_sk)
}

fn convert_ed25519_to_x25519_pub(
    publickey: &rust_ipfs::PublicKey,
) -> anyhow::Result<x25519_dalek::PublicKey> {
    let ed25519_pubkey = publickey.clone().try_into_ed25519()?;
    let ep = CompressedEdwardsY(ed25519_pubkey.to_bytes())
        .decompress()
        .ok_or(anyhow::anyhow!("Unsupported"))?;
    let mon = ep.to_montgomery();
    Ok(x25519_dalek::PublicKey::from(mon.0))
}

pub(crate) fn ecdh_encrypt(
    keypair: &rust_ipfs::Keypair,
    public_key: Option<&rust_ipfs::PublicKey>,
    data: impl AsRef<[u8]>,
) -> anyhow::Result<Vec<u8>> {
    let ed25519_keypair = keypair.clone().try_into_ed25519()?;
    let xpriv = convert_ed25519_to_x25519_priv(&ed25519_keypair.secret());
    let xpub = match public_key {
        Some(pubkey) => convert_ed25519_to_x25519_pub(pubkey)?,
        None => x25519_dalek::PublicKey::from(&xpriv),
    };

    let shared_key = xpriv.diffie_hellman(&xpub);

    encrypt(data.as_ref(), shared_key.as_bytes())
}

pub(crate) fn ecdh_decrypt(
    keypair: &rust_ipfs::Keypair,
    public_key: Option<&rust_ipfs::PublicKey>,
    data: impl AsRef<[u8]>,
) -> anyhow::Result<Vec<u8>> {
    let ed25519_keypair = keypair.clone().try_into_ed25519()?;
    let xpriv = convert_ed25519_to_x25519_priv(&ed25519_keypair.secret());
    let xpub = match public_key {
        Some(pubkey) => convert_ed25519_to_x25519_pub(pubkey)?,
        None => x25519_dalek::PublicKey::from(&xpriv),
    };

    let shared_key = xpriv.diffie_hellman(&xpub);

    decrypt(data.as_ref(), shared_key.as_bytes())
}

pub(crate) trait Signer {
    fn sign(&self, _: &Keypair) -> anyhow::Result<Vec<u8>>;
}

impl<B: AsRef<[u8]>> Signer for B {
    fn sign(&self, keypair: &Keypair) -> anyhow::Result<Vec<u8>> {
        let signature = keypair.sign(self.as_ref())?;
        Ok(signature)
    }
}

pub(crate) trait PeerIdExt {
    fn to_public_key(&self) -> Result<PublicKey, anyhow::Error>;
}

impl PeerIdExt for PeerId {
    fn to_public_key(&self) -> Result<PublicKey, anyhow::Error> {
        let multihash: Multihash = (*self).into();
        if multihash.code() != 0 {
            anyhow::bail!("PeerId does not contain inline public key");
        }
        let public_key = PublicKey::try_decode_protobuf(multihash.digest())?;
        Ok(public_key)
    }
}

#[cfg(test)]
mod test {
    use rust_ipfs::Keypair;

    use crate::{ecdh_decrypt, ecdh_encrypt};

    #[test]
    fn ecdh_encrypt_decrypt() -> anyhow::Result<()> {
        let message = b"Hello, World";

        let alice = Keypair::generate_ed25519();
        let bob = Keypair::generate_ed25519();

        let cipher = ecdh_encrypt(&alice, Some(&bob.public()), message)?;

        assert_ne!(&cipher, message);

        let plaintext = ecdh_decrypt(&bob, Some(&alice.public()), &cipher)?;

        assert_eq!(plaintext, message);

        Ok(())
    }

    #[test]
    fn ecdh_invalid_key() -> anyhow::Result<()> {
        let message = b"Hello, World";

        let alice = Keypair::generate_ed25519();
        let bob = Keypair::generate_ed25519();
        let john = Keypair::generate_ed25519();

        let cipher = ecdh_encrypt(&alice, Some(&bob.public()), message)?;

        assert_ne!(&cipher, message);

        let invalid = ecdh_decrypt(&bob, Some(&john.public()), &cipher).is_err();

        assert!(invalid);

        Ok(())
    }

    #[test]
    fn error_with_incorrect_key() -> anyhow::Result<()> {
        let message = b"Hello, World";

        let alice = Keypair::generate_ecdsa();
        let bob = Keypair::generate_secp256k1();

        let invalid = ecdh_encrypt(&alice, Some(&bob.public()), message).is_err();

        assert!(invalid);

        let invalid = ecdh_decrypt(&bob, Some(&alice.public()), message).is_err();

        assert!(invalid);

        Ok(())
    }
}

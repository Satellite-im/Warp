use crate::hash::sha256_hash;
use anyhow::Result;
use ed25519_dalek::Keypair;
use x25519_dalek::{PublicKey, StaticSecret};

pub fn x25519_key_exchange(
    prikey: &StaticSecret,
    pubkey: PublicKey,
    nonce: Option<&[u8]>,
    hashed: bool,
) -> Result<Vec<u8>> {
    let secret = prikey.diffie_hellman(&pubkey);

    let hash = match hashed {
        true => sha256_hash(secret.as_bytes(), nonce)?,
        false => secret.as_bytes().to_vec(),
    };

    Ok(hash)
}

//TODO: Evaluate and determine if this is the best choice for conversion of ed25519 to x25519
//      as it may require specific access to the curve via curve25519-dalek crate
//Note: It may be better to use two separate keypairs.
pub fn ed25519_to_x25519(keypair: &Keypair) -> Result<StaticSecret> {
    Ok(StaticSecret::from(keypair.secret.to_bytes()))
}

#[cfg(test)]
mod test {
    use crate::cipher::*;
    use crate::exchange::*;
    use ed25519_dalek::Keypair;
    use rand::rngs::OsRng;
    use x25519_dalek::{PublicKey, StaticSecret};

    #[test]
    fn key_exchange() -> anyhow::Result<()> {
        let alice_private_key = StaticSecret::new(&mut OsRng);
        let alice_public_key = PublicKey::from(&alice_private_key);

        let bob_private_key = StaticSecret::new(&mut OsRng);
        let bob_public_key = PublicKey::from(&bob_private_key);

        let a_dh = x25519_key_exchange(&alice_private_key, bob_public_key, None, true)?;
        let b_dh = x25519_key_exchange(&bob_private_key, alice_public_key, None, true)?;

        assert_eq!(a_dh, b_dh);

        Ok(())
    }

    #[test]
    fn ed25519_key_exchange() -> anyhow::Result<()> {
        let alice_keypair = Keypair::generate(&mut OsRng);
        let bob_keypair = Keypair::generate(&mut OsRng);

        let alice_secret = ed25519_to_x25519(&alice_keypair)?;
        let alice_pubkey = PublicKey::from(&alice_secret);

        let bob_secret = ed25519_to_x25519(&bob_keypair)?;
        let bob_pubkey = PublicKey::from(&bob_secret);

        let a_dh = x25519_key_exchange(&alice_secret, bob_pubkey, None, true)?;
        let b_dh = x25519_key_exchange(&bob_secret, alice_pubkey, None, true)?;

        assert_eq!(a_dh, b_dh);

        Ok(())
    }

    #[test]
    fn key_exchange_encryption() -> anyhow::Result<()> {
        let alice_keypair = Keypair::generate(&mut OsRng);
        let bob_keypair = Keypair::generate(&mut OsRng);

        let alice_private_key = ed25519_to_x25519(&alice_keypair)?;
        let alice_public_key = PublicKey::from(&alice_private_key);

        let bob_private_key = ed25519_to_x25519(&bob_keypair)?;
        let bob_public_key = PublicKey::from(&bob_private_key);

        let a_dh = x25519_key_exchange(&alice_private_key, bob_public_key, None, true)?;
        let b_dh = x25519_key_exchange(&bob_private_key, alice_public_key, None, true)?;
        assert_eq!(a_dh, b_dh);

        {
            let for_bob = aes256gcm_encrypt(&a_dh, &b"Hello Bob"[..])?;
            let plaintext = aes256gcm_decrypt(&b_dh, &for_bob)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Bob"));
        }

        {
            let for_alice = aes256gcm_encrypt(&a_dh, &b"Hello Alice"[..])?;
            let plaintext = aes256gcm_decrypt(&b_dh, &for_alice)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Alice"));
        }

        Ok(())
    }

    #[test]
    fn ed25519_key_exchange_encryption() -> anyhow::Result<()> {
        let alice_private_key = StaticSecret::new(&mut OsRng);
        let alice_public_key = PublicKey::from(&alice_private_key);

        let bob_private_key = StaticSecret::new(&mut OsRng);
        let bob_public_key = PublicKey::from(&bob_private_key);

        let a_dh = x25519_key_exchange(&alice_private_key, bob_public_key, None, true)?;
        let b_dh = x25519_key_exchange(&bob_private_key, alice_public_key, None, true)?;
        assert_eq!(a_dh, b_dh);

        {
            let for_bob = aes256gcm_encrypt(&a_dh, &b"Hello Bob"[..])?;
            let plaintext = aes256gcm_decrypt(&b_dh, &for_bob)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Bob"));
        }

        {
            let for_alice = aes256gcm_encrypt(&a_dh, &b"Hello Alice"[..])?;
            let plaintext = aes256gcm_decrypt(&b_dh, &for_alice)
                .map(|ptxt| String::from_utf8_lossy(&ptxt).to_string())?;
            assert_eq!(plaintext, String::from("Hello Alice"));
        }

        Ok(())
    }
}

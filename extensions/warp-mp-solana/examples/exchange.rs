use warp::crypto::exchange::X25519PublicKey;
use warp::multipass::identity::{Identity, PublicKey};
use warp::multipass::MultiPass;
use warp::tesseract::Tesseract;
use warp_mp_solana::solana::anchor_client::anchor_lang::prelude::Pubkey;
use warp_mp_solana::SolanaAccount;

fn account() -> anyhow::Result<SolanaAccount> {
    let mut tesseract = Tesseract::default();
    tesseract
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    let mut account = SolanaAccount::with_devnet(&tesseract);
    account.create_identity(None, None)?;
    Ok(account)
}

fn username(ident: &Identity) -> String {
    format!("{}#{}", &ident.username(), &ident.short_id())
}

fn main() -> anyhow::Result<()> {
    let account_a = account()?;
    let account_b = account()?;

    let ident_a = account_a.get_own_identity()?;
    println!(
        "{} with {}",
        username(&ident_a),
        Pubkey::new(ident_a.public_key().as_ref())
    );

    let ident_b = account_b.get_own_identity()?;
    println!(
        "{} with {}",
        username(&ident_b),
        Pubkey::new(ident_b.public_key().as_ref())
    );

    let alice_pubkey = ecdh_public_key(&account_a)?;
    let bob_pubkey = ecdh_public_key(&account_b)?;

    let alice_key = ecdh_key_exchange(&account_a, bob_pubkey).map(hex::encode)?;
    let bob_key = ecdh_key_exchange(&account_b, alice_pubkey).map(hex::encode)?;

    assert_eq!(&alice_key, &bob_key);

    println!("Account A Key: {}", alice_key);
    println!("Account B Key: {}", bob_key);

    //TODO: Encryption?

    Ok(())
}

fn ecdh_public_key(account: &impl MultiPass) -> anyhow::Result<PublicKey> {
    let privkey = account.decrypt_private_key(None)?;
    let keypair = warp::crypto::signature::Ed25519Keypair::from_bytes(&privkey)?;
    let secret = warp::crypto::exchange::X25519Secret::from_ed25519_keypair(&keypair)?;
    let pubkey = PublicKey::from_bytes(secret.public_key().to_inner().as_bytes());
    Ok(pubkey)
}

fn ecdh_key_exchange(account: &impl MultiPass, public_key: PublicKey) -> anyhow::Result<Vec<u8>> {
    let private_key = account.decrypt_private_key(None)?;
    let kp = warp::crypto::signature::Ed25519Keypair::from_bytes(&private_key)?;
    let secret = warp::crypto::exchange::X25519Secret::from_ed25519_keypair(&kp)?;
    let pubkey = X25519PublicKey::from_bytes(public_key.as_ref());
    let ecdh_key = secret.key_exchange(pubkey);
    Ok(ecdh_key)
}

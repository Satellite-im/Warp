use warp_solana_utils::anchor_client::solana_sdk::signature::Signer;
use warp_solana_utils::wallet::{PhraseType, SolanaWallet};

fn main() -> warp_common::anyhow::Result<()> {
    let wallet = SolanaWallet::create_random(PhraseType::Standard, None)?;
    let kp = wallet.get_keypair()?;
    println!("Solana private key: {}", kp.to_base58_string());
    println!("Solana public key: {}", kp.pubkey());
    println!(
        "Mnemonic Phrase (Standard): {}",
        wallet.get_mnemonic_phrase()?
    );
    Ok(())
}

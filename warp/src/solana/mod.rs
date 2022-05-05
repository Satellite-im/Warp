// Based on https://github.com/Satellite-im/Core-PWA/blob/dev/libraries/Solana/SolanaManager/SolanaManager.ts
// Note: This is a reference for `warp-mp-solana`.

pub mod error;
pub mod helper;
pub mod manager;
pub mod wallet;

pub use anchor_client;
pub use bs58;

use anchor_client::solana_sdk::pubkey::Pubkey;
use anyhow::{anyhow, Result};

pub fn pubkey_from_seed<S: AsRef<str>>(
    seed_key: &Pubkey,
    seed: S,
    id: &Pubkey,
) -> Result<(Pubkey, Pubkey)> {
    pubkey_from_seeds(&[&seed_key.to_bytes()], seed, id)
}

pub fn pubkey_from_seeds<S: AsRef<str>>(
    seeds: &[&[u8]],
    seed: S,
    id: &Pubkey,
) -> Result<(Pubkey, Pubkey)> {
    let (base_pkey, _) = Pubkey::try_find_program_address(seeds, id)
        .ok_or_else(|| anyhow!("Error finding program"))?;
    let pkey = Pubkey::create_with_seed(&base_pkey, seed.as_ref(), id)?;
    Ok((base_pkey, pkey))
}

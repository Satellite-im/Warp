// Based on https://github.com/Satellite-im/Core-PWA/blob/dev/libraries/Solana/SolanaManager/SolanaManager.ts
// Note: This is a reference for `warp-mp-solana`.

pub mod error;
#[cfg(not(target_arch = "wasm32"))]
pub mod helper;
#[cfg(not(target_arch = "wasm32"))]
pub mod manager;
pub mod wallet;

#[cfg(not(target_arch = "wasm32"))]
pub use anchor_client;

#[cfg(not(target_arch = "wasm32"))]
pub use anchor_client::solana_sdk::pubkey::Pubkey;

pub use bs58;

#[cfg(not(target_arch = "wasm32"))]
use anyhow::{anyhow, Result};
#[cfg(target_arch = "wasm32")]
use solana_sdk::pubkey::Pubkey;

#[derive(Copy, Clone, Debug)]
pub struct ProgramAddressKeys {
    base: Pubkey,
    pkey: Pubkey,
}

impl ProgramAddressKeys {
    pub fn base(&self) -> Pubkey {
        self.base
    }

    pub fn pkey(&self) -> Pubkey {
        self.pkey
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn pubkey_from_seed(seed_key: Pubkey, seed: &str, id: Pubkey) -> Result<(Pubkey, Pubkey)> {
    pubkey_from_seeds(&[&seed_key.to_bytes()], seed, id)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn pubkey_from_seeds(seeds: &[&[u8]], seed: &str, id: Pubkey) -> Result<(Pubkey, Pubkey)> {
    let (base_pkey, _) = Pubkey::try_find_program_address(seeds, &id)
        .ok_or_else(|| anyhow!("Error finding program"))?;
    let pkey = Pubkey::create_with_seed(&base_pkey, seed, &id)?;
    Ok((base_pkey, pkey))
}

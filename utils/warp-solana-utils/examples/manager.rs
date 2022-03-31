use warp_solana_utils::manager::SolanaManager;

fn main() -> warp_common::anyhow::Result<()> {
    let mut manager = SolanaManager::new();
    manager.initialize_random()?;

    println!("{:?}", manager);

    Ok(())
}

use warp_solana_utils::wallet::SolanaWallet;

fn main() -> warp_common::anyhow::Result<()> {
    let wallet = SolanaWallet::restore_from_mnemonic(
        None,
        "morning caution dose lab six actress pond humble pause enact virtual train",
    )?;

    println!("{:?}", wallet);

    Ok(())
}

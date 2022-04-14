#[cfg(test)]
mod tests {
    #[allow(unused)]
    use std::str::FromStr;
    use std::sync::{Arc, Mutex};
    use warp_common::anyhow;
    use warp_mp_solana::Account;
    #[allow(unused)]
    use warp_multipass::identity::{Identifier, IdentityUpdate, PublicKey};
    use warp_multipass::MultiPass;
    use warp_solana_utils::anchor_client::anchor_lang::prelude::Pubkey;
    use warp_solana_utils::wallet::{PhraseType, SolanaWallet};
    use warp_tesseract::Tesseract;

    #[allow(unused)]
    fn pregenerated_wallet() -> anyhow::Result<SolanaWallet> {
        SolanaWallet::restore_from_mnemonic(
            None,
            "morning caution dose lab six actress pond humble pause enact virtual train",
        )
    }

    #[allow(unused)]
    fn generated_wallet() -> anyhow::Result<SolanaWallet> {
        SolanaWallet::create_random(PhraseType::Standard, None)
    }

    fn tesseract_with_random_key() -> anyhow::Result<Arc<Mutex<Tesseract>>> {
        let mut tesseract = Tesseract::default();
        let key = warp_crypto::generate(32);
        tesseract.unlock(&key)?;
        Ok(Arc::new(Mutex::new(tesseract)))
    }

    #[allow(unused)]
    fn tesseract_with_preset_key() -> anyhow::Result<Arc<Mutex<Tesseract>>> {
        let mut tesseract = Tesseract::default();
        tesseract.unlock(
            b"this is my totally secured password that should nnever be embedded in code",
        )?;
        Ok(Arc::new(Mutex::new(tesseract)))
    }

    #[test]
    fn use_mp_with_pregenerated_wallet() -> anyhow::Result<()> {
        let mut account = Account::with_devnet();
        let tesseract = tesseract_with_random_key()?;
        account.set_tesseract(tesseract);
        account.insert_solana_wallet(pregenerated_wallet()?)?;

        let ident = account.get_own_identity()?;

        let pubkey = Pubkey::new(ident.public_key.to_bytes());

        assert_eq!(
            pubkey.to_string(),
            "68vtRPQcsV7ruWXa6Z8Enrb6TsXhbRzMywgCnEVyk7Va"
        );

        Ok(())
    }

    //TODO: Add a skip when there is an error

    // #[test]
    // fn use_mp_with_new_wallet() -> anyhow::Result<()> {
    //     let mut account = Account::with_devnet();
    //     let tesseract = tesseract_with_random_key()?;
    //     account.set_tesseract(tesseract);
    //
    //     account.create_identity("RandomUser", "")?;
    //
    //     let ident = account.get_own_identity()?;
    //
    //     let pubkey = Pubkey::new(ident.public_key.to_bytes());
    //
    //     assert_ne!(
    //         pubkey.to_string(),
    //         "68vtRPQcsV7ruWXa6Z8Enrb6TsXhbRzMywgCnEVyk7Va"
    //     );
    //
    //     Ok(())
    // }
    //
    // #[test]
    // fn use_mp_to_find_account() -> anyhow::Result<()> {
    //     let mut account = Account::with_devnet();
    //     let tesseract = tesseract_with_random_key()?;
    //     account.set_tesseract(tesseract);
    //
    //     account.create_identity("RandomUser", "")?;
    //
    //     let real_pubkey = Pubkey::from_str("68vtRPQcsV7ruWXa6Z8Enrb6TsXhbRzMywgCnEVyk7Va")?;
    //
    //     let _ = account.get_identity(Identifier::PublicKey(PublicKey::from_bytes(
    //         &real_pubkey.to_bytes(),
    //     )))?;
    //
    //     //TODO: Assert test here
    //
    //     Ok(())
    // }
}

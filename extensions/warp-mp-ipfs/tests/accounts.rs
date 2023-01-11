#[cfg(test)]
mod test {
    use std::time::Duration;

    use warp::crypto::DID;
    use warp::multipass::identity::{Identity, IdentityUpdate};
    use warp::multipass::MultiPass;
    use warp::tesseract::Tesseract;
    use warp_mp_ipfs::config::Discovery;
    use warp_mp_ipfs::ipfs_identity_temporary;

    async fn create_account(
        username: Option<&str>,
        passphrase: Option<&str>,
        context: Option<String>,
    ) -> anyhow::Result<(Box<dyn MultiPass>, DID, Identity)> {
        let tesseract = Tesseract::default();
        tesseract.unlock(b"internal pass").unwrap();
        let mut config = warp_mp_ipfs::config::MpIpfsConfig::development();
        config.store_setting.discovery = Discovery::Provider(context);
        let mut account = ipfs_identity_temporary(Some(config), tesseract, None).await?;
        let did = account.create_identity(username, passphrase).await?;
        let identity = account.get_own_identity().await?;
        Ok((Box::new(account), did, identity))
    }

    #[tokio::test]
    async fn create_identity() -> anyhow::Result<()> {
        let (_, did, _) = create_account(
            None,
            Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            None
        )
        .await?;

        assert_eq!(
            did.to_string(),
            "did:key:z6MksiU5wFcZHHSp4VvtQePW4zwUDNmGADqxfQi4TdcEvmjz"
        );

        Ok(())
    }

    #[tokio::test]
    async fn get_own_identity() -> anyhow::Result<()> {
        let (_, _, identity) = create_account(
            Some("JohnDoe"),
            Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            None
        )
        .await?;

        assert_eq!(identity.username(), "JohnDoe");
        assert_eq!(
            identity.did_key().to_string(),
            "did:key:z6MksiU5wFcZHHSp4VvtQePW4zwUDNmGADqxfQi4TdcEvmjz"
        );

        Ok(())
    }

    #[tokio::test]
    async fn get_identity() -> anyhow::Result<()> {
        let (account_a, _, _) = create_account(
            Some("JohnDoe"),
            Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            Some("test::get_identity".into())
        )
        .await?;

        let (_, did_b, _) = create_account(Some("JaneDoe"), None, Some("test::get_identity".into())).await?;

        //used to wait for the nodes to discover eachother and provide their identity to each other
        let identity_b = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Ok(Some(id)) = account_a
                    .get_identity(did_b.clone().into())
                    .await
                    .map(|s| s.first().cloned())
                {
                    break id;
                }
            }
        })
        .await?;

        assert_eq!(identity_b.username(), "JaneDoe");

        Ok(())
    }

    #[tokio::test]
    async fn get_identity_by_username() -> anyhow::Result<()> {
        let (account_a, _, _) = create_account(Some("JohnDoe"), None, Some("test::get_identity_by_username".into())).await?;

        let (_account_b, _, _) = create_account(Some("JaneDoe"), None, Some("test::get_identity_by_username".into())).await?;

        //used to wait for the nodes to discover eachother and provide their identity to each other

        let identity_b = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(id) = account_a
                    .get_identity(String::from("JaneDoe").into())
                    .await
                    .expect("Should not fail")
                    .first()
                    .cloned()
                {
                    break id;
                }
            }
        })
        .await?;

        assert_eq!(identity_b.username(), "JaneDoe");
        Ok(())
    }

    #[tokio::test]
    async fn update_identity_username() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        tesseract.unlock(b"internal pass").unwrap();

        let mut account = ipfs_identity_temporary(None, tesseract, None).await?;
        account
            .create_identity(
                Some("JohnDoe"),
                Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            )
            .await?;

        let old_identity = account.get_own_identity().await?;

        account
            .update_identity(IdentityUpdate::set_username("JohnDoe2.0".into()))
            .await?;

        let updated_identity = account.get_own_identity().await?;

        assert_ne!(old_identity.username(), updated_identity.username());

        Ok(())
    }

    #[tokio::test]
    async fn update_identity_status() -> anyhow::Result<()> {
        let tesseract = Tesseract::default();
        tesseract.unlock(b"internal pass").unwrap();

        let mut account = ipfs_identity_temporary(None, tesseract, None).await?;
        account
            .create_identity(
                Some("JohnDoe"),
                Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            )
            .await?;

        let old_identity = account.get_own_identity().await?;

        account
            .update_identity(IdentityUpdate::set_status_message(Some("Blast off".into())))
            .await?;

        let updated_identity = account.get_own_identity().await?;

        assert_eq!(old_identity.status_message(), None);

        assert_eq!(updated_identity.status_message(), Some("Blast off".into()));

        Ok(())
    }
}

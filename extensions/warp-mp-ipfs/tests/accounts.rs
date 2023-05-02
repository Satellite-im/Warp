
pub mod common;
#[cfg(test)]
mod test {
    use std::time::Duration;

    use warp::multipass::identity::{IdentityStatus, IdentityUpdate, Platform};
    use warp::multipass::MultiPass;
    use warp::tesseract::Tesseract;
    use warp_mp_ipfs::ipfs_identity_temporary;
    use crate::common::{create_accounts, create_account};

    #[tokio::test]
    async fn create_identity() -> anyhow::Result<()> {
        let (_, did, _) = create_account(
            None,
            Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            None,
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
            None,
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
        let accounts =
            create_accounts(vec![(
            Some("JohnDoe"),
            Some("morning caution dose lab six actress pond humble pause enact virtual train"),
            Some("test::get_identity".into()),
        ), (Some("JaneDoe"), None, Some("test::get_identity".into()))])
            .await?;

        let (account_a, _, _) = accounts.first().expect("Account exist");

        let (_, did_b, _) = accounts.last().expect("Account exist");

        //used to wait for the nodes to discover eachother and provide their identity to each other
        let identity_b = tokio::time::timeout(Duration::from_secs(60), async {
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
        let accounts = create_accounts(vec![
            (
                Some("JohnDoe"),
                None,
                Some("test::get_identity_by_username".into()),
            ),
            (
                Some("JaneDoe"),
                None,
                Some("test::get_identity_by_username".into()),
            ),
        ])
        .await?;

        let (account_a, _, _) = accounts.first().unwrap();

        let (_account_b, _, _) = accounts.last().unwrap();

        //used to wait for the nodes to discover eachother and provide their identity to each other

        let identity_b = tokio::time::timeout(Duration::from_secs(60), async {
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
            .update_identity(IdentityUpdate::Username("JohnDoe2.0".into()))
            .await?;

        let updated_identity = account.get_own_identity().await?;

        assert_ne!(old_identity.username(), updated_identity.username());

        Ok(())
    }

    #[tokio::test]
    async fn update_identity_status_message() -> anyhow::Result<()> {
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
            .update_identity(IdentityUpdate::StatusMessage(Some("Blast off".into())))
            .await?;

        let updated_identity = account.get_own_identity().await?;

        assert_eq!(old_identity.status_message(), None);

        assert_eq!(updated_identity.status_message(), Some("Blast off".into()));

        Ok(())
    }

    #[tokio::test]
    async fn identity_status() -> anyhow::Result<()> {
        let (account, did, _) =
            create_account(Some("JohnDoe"), None, Some("test::identity_status".into())).await?;
        let status = account.identity_status(&did).await?;
        assert_eq!(status, IdentityStatus::Online);
        Ok(())
    }

    #[tokio::test]
    async fn update_identity_status() -> anyhow::Result<()> {
        let (mut account, did, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::update_identity_status".into()),
        )
        .await?;
        let status = account.identity_status(&did).await?;
        assert_eq!(status, IdentityStatus::Online);

        account.set_identity_status(IdentityStatus::Away).await?;

        let status = account.identity_status(&did).await?;
        assert_eq!(status, IdentityStatus::Away);

        Ok(())
    }

    #[tokio::test]
    async fn get_identity_status() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                Some("JohnDoe"),
                None,
                Some("test::get_identity_status".into()),
            ),
            (
                Some("JaneDoe"),
                None,
                Some("test::get_identity_status".into()),
            ),
        ])
        .await?;

        let (account_a, _, _) = accounts.first().unwrap();

        let (mut account_b, did_b, _) = accounts.last().cloned().unwrap();

        let status_b = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(status) = account_a.identity_status(&did_b).await {
                    break status;
                }
            }
        })
        .await?;

        assert_eq!(status_b, IdentityStatus::Online);

        account_b.set_identity_status(IdentityStatus::Away).await?;

        let status = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(status) = account_a.identity_status(&did_b).await {
                    if status != status_b {
                        break status;
                    }
                }
            }
        })
        .await?;

        assert_eq!(status, IdentityStatus::Away);

        Ok(())
    }

    #[tokio::test]
    async fn identity_platform() -> anyhow::Result<()> {
        let (account, did, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::identity_platform".into()),
        )
        .await?;
        let platform = account.identity_platform(&did).await?;
        assert_eq!(platform, Platform::Desktop);
        Ok(())
    }

    #[tokio::test]
    async fn get_identity_platform() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                Some("JohnDoe"),
                None,
                Some("test::get_identity_platform".into()),
            ),
            (
                Some("JaneDoe"),
                None,
                Some("test::get_identity_platform".into()),
            ),
        ])
        .await?;

        let (account_a, _, _) = accounts.first().unwrap();

        let (_account_b, did_b, _) = accounts.last().unwrap();

        tokio::time::sleep(Duration::from_secs(1)).await;

        let platform_b = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Ok(platform) = account_a.identity_platform(did_b).await {
                    break platform;
                }
            }
        })
        .await?;

        assert_eq!(platform_b, Platform::Desktop);
        Ok(())
    }
}

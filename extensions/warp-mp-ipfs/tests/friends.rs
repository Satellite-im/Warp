#[cfg(test)]
mod test {
    use std::time::Duration;

    use futures::StreamExt;
    use warp::crypto::DID;
    use warp::multipass::identity::Identity;
    use warp::multipass::{MultiPass, MultiPassEventKind};
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
        config.store_setting.use_phonebook = false;
        let mut account = ipfs_identity_temporary(Some(config), tesseract, None).await?;
        let did = account.create_identity(username, passphrase).await?;
        let identity = account.get_own_identity().await?;
        Ok((Box::new(account), did, identity))
    }

    #[tokio::test]
    async fn add_friend() -> anyhow::Result<()> {
        let (mut account_a, did_a, _) =
            create_account(Some("JohnDoe"), None, Some("test::add_friend".into())).await?;

        let (mut account_b, did_b, _) =
            create_account(Some("JaneDoe"), None, Some("test::add_friend".into())).await?;

        let mut subscribe_a = account_a.subscribe().await?;
        let mut subscribe_b = account_b.subscribe().await?;
        account_a.send_request(&did_b).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            let did = loop {
                if let Some(MultiPassEventKind::FriendRequestReceived { from }) =
                    subscribe_b.next().await
                {
                    break from;
                }
            };
            account_b.accept_request(&did).await
        })
        .await??;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::FriendAdded { .. }) = subscribe_a.next().await {
                    break;
                }
            }
        })
        .await?;

        assert!(account_b.has_friend(&did_a).await?);
        assert!(account_a.has_friend(&did_b).await?);
        Ok(())
    }

    #[tokio::test]
    async fn remove_friend() -> anyhow::Result<()> {
        let (mut account_a, did_a, _) =
            create_account(Some("JohnDoe"), None, Some("test::remove_friend".into())).await?;

        let (mut account_b, did_b, _) =
            create_account(Some("JaneDoe"), None, Some("test::remove_friend".into())).await?;

        let mut subscribe_a = account_a.subscribe().await?;
        let mut subscribe_b = account_b.subscribe().await?;

        account_a.send_request(&did_b).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            let did = loop {
                if let Some(MultiPassEventKind::FriendRequestReceived { from }) =
                    subscribe_b.next().await
                {
                    break from;
                }
            };
            account_b.accept_request(&did).await
        })
        .await??;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::FriendAdded { .. }) = subscribe_a.next().await {
                    break;
                }
            }
        })
        .await?;

        assert!(account_b.has_friend(&did_a).await?);
        assert!(account_a.has_friend(&did_b).await?);

        account_a.remove_friend(&did_b).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::FriendRemoved { .. }) = subscribe_a.next().await {
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::FriendRemoved { .. }) = subscribe_b.next().await {
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn reject_friend() -> anyhow::Result<()> {
        let (mut account_a, _, _) =
            create_account(Some("JohnDoe"), None, Some("test::reject_friend".into())).await?;

        let (mut account_b, did_b, _) =
            create_account(Some("JaneDoe"), None, Some("test::reject_friend".into())).await?;

        let mut subscribe_a = account_a.subscribe().await?;
        let mut subscribe_b = account_b.subscribe().await?;

        account_a.send_request(&did_b).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            let did = loop {
                if let Some(MultiPassEventKind::FriendRequestReceived { from }) =
                    subscribe_b.next().await
                {
                    break from;
                }
            };
            account_b.deny_request(&did).await
        })
        .await??;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::OutgoingFriendRequestRejected { .. }) =
                    subscribe_a.next().await
                {
                    break;
                }
            }
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn close_request() -> anyhow::Result<()> {
        let (mut account_a, _, _) =
            create_account(Some("JohnDoe"), None, Some("test::close_request".into())).await?;

        let (mut account_b, did_b, _) =
            create_account(Some("JaneDoe"), None, Some("test::close_request".into())).await?;
        let mut subscribe_a = account_a.subscribe().await?;
        let mut subscribe_b = account_b.subscribe().await?;

        account_a.send_request(&did_b).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::FriendRequestReceived { .. }) =
                    subscribe_b.next().await
                {
                    break;
                }
            }
        })
        .await?;

        account_a.close_request(&did_b).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::IncomingFriendRequestClosed { .. }) =
                    subscribe_b.next().await
                {
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::OutgoingFriendRequestClosed { .. }) =
                    subscribe_a.next().await
                {
                    break;
                }
            }
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn incoming_request() -> anyhow::Result<()> {
        let (mut account_a, did_a, _) =
            create_account(Some("JohnDoe"), None, Some("test::incoming_request".into())).await?;

        let (mut account_b, did_b, _) =
            create_account(Some("JaneDoe"), None, Some("test::incoming_request".into())).await?;
        let mut subscribe_a = account_a.subscribe().await?;
        let mut subscribe_b = account_b.subscribe().await?;

        account_a.send_request(&did_b).await?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::FriendRequestSent { .. }) = subscribe_a.next().await
                {
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::FriendRequestReceived { .. }) =
                    subscribe_b.next().await
                {
                    break;
                }
            }
        })
        .await?;

        let list = account_b.list_incoming_request().await?;

        assert!(list.contains(&did_a));

        Ok(())
    }

    #[tokio::test]
    async fn outgoing_request() -> anyhow::Result<()> {
        let (mut account_a, _, _) =
            create_account(Some("JohnDoe"), None, Some("test::outgoing_request".into())).await?;

        let (_account_b, did_b, _) =
            create_account(Some("JaneDoe"), None, Some("test::outgoing_request".into())).await?;
        let mut subscribe_a = account_a.subscribe().await?;

        account_a.send_request(&did_b).await?;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MultiPassEventKind::FriendRequestSent { .. }) = subscribe_a.next().await
                {
                    break;
                }
            }
        })
        .await?;

        let list = account_a.list_outgoing_request().await?;

        assert!(list.contains(&did_b));

        Ok(())
    }

    #[tokio::test]
    async fn block_unblock_identity() -> anyhow::Result<()> {
        let (mut account_a, did_a, _) =
            create_account(None, None, Some("test::block_unblock_identity".into())).await?;

        let (mut account_b, did_b, _) =
            create_account(None, None, Some("test::block_unblock_identity".into())).await?;

        let mut subscribe_a = account_a.subscribe().await?;
        let mut subscribe_b = account_b.subscribe().await?;

        account_a.block(&did_b).await?;
        account_b.block(&did_a).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(
                    MultiPassEventKind::Blocked { did } | MultiPassEventKind::BlockedBy { did },
                ) = subscribe_a.next().await
                {
                    assert_eq!(did, did_b);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(
                    MultiPassEventKind::Blocked { did } | MultiPassEventKind::BlockedBy { did },
                ) = subscribe_b.next().await
                {
                    assert_eq!(did, did_a);
                    break;
                }
            }
        })
        .await?;

        account_a.unblock(&did_b).await?;
        account_b.unblock(&did_a).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(
                    MultiPassEventKind::Unblocked { did } | MultiPassEventKind::UnblockedBy { did },
                ) = subscribe_a.next().await
                {
                    assert_eq!(did, did_b);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(
                    MultiPassEventKind::Unblocked { did } | MultiPassEventKind::UnblockedBy { did },
                ) = subscribe_b.next().await
                {
                    assert_eq!(did, did_a);
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn cannot_block_self() -> anyhow::Result<()> {
        let (mut account_a, did_a, _) = create_account(
            Some("JohnDoe"),
            None,
            Some("test::cannot_block_self".into()),
        )
        .await?;

        assert!(account_a.block(&did_a).await.is_err());

        Ok(())
    }
}

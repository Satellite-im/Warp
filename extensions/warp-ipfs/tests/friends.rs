pub mod common;
#[cfg(test)]
mod test {
    use std::time::Duration;

    use crate::common::{create_account, create_accounts};
    use futures::StreamExt;
    use warp::multipass::{Friends, MultiPassEvent, MultiPassEventKind};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;

    #[async_test]
    async fn add_friend() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (Some("JohnDoe"), None, Some("test::add_friend".into())),
            (Some("JaneDoe"), None, Some("test::add_friend".into())),
        ])
        .await?;

        let (mut account_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut account_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut subscribe_a = account_a.multipass_subscribe().await?;
        let mut subscribe_b = account_b.multipass_subscribe().await?;
        account_a.send_request(&did_b).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            let did = loop {
                if let Some(MultiPassEventKind::FriendRequestReceived { from, .. }) =
                    subscribe_b.next().await
                {
                    break from;
                }
            };
            account_b.accept_request(&did).await
        })
        .await??;

        crate::common::timeout(Duration::from_secs(60), async {
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

    #[async_test]
    async fn remove_friend() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (Some("JohnDoe"), None, Some("test::remove_friend".into())),
            (Some("JaneDoe"), None, Some("test::remove_friend".into())),
        ])
        .await?;

        let (mut account_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut account_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut subscribe_a = account_a.multipass_subscribe().await?;
        let mut subscribe_b = account_b.multipass_subscribe().await?;

        account_a.send_request(&did_b).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            let did = loop {
                if let Some(MultiPassEventKind::FriendRequestReceived { from, .. }) =
                    subscribe_b.next().await
                {
                    break from;
                }
            };
            account_b.accept_request(&did).await
        })
        .await??;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::FriendRemoved { .. }) = subscribe_a.next().await {
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::FriendRemoved { .. }) = subscribe_b.next().await {
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[async_test]
    async fn reject_friend() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (Some("JohnDoe"), None, Some("test::reject_friend".into())),
            (Some("JaneDoe"), None, Some("test::reject_friend".into())),
        ])
        .await?;

        let (mut account_a, _, _) = accounts.first().cloned().unwrap();
        let (mut account_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut subscribe_a = account_a.multipass_subscribe().await?;
        let mut subscribe_b = account_b.multipass_subscribe().await?;

        account_a.send_request(&did_b).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            let did = loop {
                if let Some(MultiPassEventKind::FriendRequestReceived { from, .. }) =
                    subscribe_b.next().await
                {
                    break from;
                }
            };
            account_b.deny_request(&did).await
        })
        .await??;

        crate::common::timeout(Duration::from_secs(60), async {
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

    #[async_test]
    async fn close_request() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (Some("JohnDoe"), None, Some("test::close_request".into())),
            (Some("JaneDoe"), None, Some("test::close_request".into())),
        ])
        .await?;

        let (mut account_a, _, _) = accounts.first().cloned().unwrap();
        let (mut account_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut subscribe_a = account_a.multipass_subscribe().await?;
        let mut subscribe_b = account_b.multipass_subscribe().await?;

        account_a.send_request(&did_b).await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::IncomingFriendRequestClosed { .. }) =
                    subscribe_b.next().await
                {
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

    #[async_test]
    async fn incoming_request() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (Some("JohnDoe"), None, Some("test::incoming_request".into())),
            (Some("JaneDoe"), None, Some("test::incoming_request".into())),
        ])
        .await?;

        let (mut account_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut account_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut subscribe_a = account_a.multipass_subscribe().await?;
        let mut subscribe_b = account_b.multipass_subscribe().await?;

        account_a.send_request(&did_b).await?;
        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::FriendRequestSent { .. }) = subscribe_a.next().await
                {
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        assert!(list.iter().any(|request| request.identity().eq(&did_a)));

        Ok(())
    }

    #[async_test]
    async fn outgoing_request() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (Some("JohnDoe"), None, Some("test::outgoing_request".into())),
            (Some("JaneDoe"), None, Some("test::outgoing_request".into())),
        ])
        .await?;

        let (mut account_a, _, _) = accounts.first().cloned().unwrap();
        let (_, did_b, _) = accounts.last().cloned().unwrap();

        let mut subscribe_a = account_a.multipass_subscribe().await?;

        account_a.send_request(&did_b).await?;
        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::FriendRequestSent { .. }) = subscribe_a.next().await
                {
                    break;
                }
            }
        })
        .await?;

        let list = account_a.list_outgoing_request().await?;

        assert!(list.iter().any(|request| request.identity().eq(&did_b)));
        Ok(())
    }

    #[async_test]
    async fn block_unblock_identity() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                Some("JohnDoe"),
                None,
                Some("test::block_unblock_identity".into()),
            ),
            (
                Some("JaneDoe"),
                None,
                Some("test::block_unblock_identity".into()),
            ),
        ])
        .await?;

        let (mut account_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut account_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut subscribe_a = account_a.multipass_subscribe().await?;
        let mut subscribe_b = account_b.multipass_subscribe().await?;

        account_a.block(&did_b).await?;
        account_b.block(&did_a).await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

    #[async_test]
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

#[cfg(test)]
mod test {
    use std::time::Duration;

    use futures::StreamExt;
    use warp::crypto::DID;
    use warp::multipass::identity::Identity;
    use warp::multipass::MultiPass;
    use warp::raygun::{ConversationType, MessageEventKind, RayGun, RayGunEventKind};
    use warp::tesseract::Tesseract;
    use warp_mp_ipfs::config::Discovery;
    use warp_mp_ipfs::ipfs_identity_temporary;
    use warp_rg_ipfs::{IpfsMessaging, Temporary};

    async fn create_account_and_chat(
        username: Option<&str>,
        passphrase: Option<&str>,
        context: Option<String>,
    ) -> anyhow::Result<(Box<dyn MultiPass>, Box<dyn RayGun>, DID, Identity)> {
        let tesseract = Tesseract::default();
        tesseract.unlock(b"internal pass").unwrap();
        let mut config = warp_mp_ipfs::config::MpIpfsConfig::development();
        config.store_setting.discovery = Discovery::Provider(context);
        config.store_setting.broadcast_interval = 100;

        let mut account = Box::new(ipfs_identity_temporary(Some(config), tesseract, None).await?)
            as Box<dyn MultiPass>;
        let did = account.create_identity(username, passphrase).await?;
        let identity = account.get_own_identity().await?;

        let raygun =
            Box::new(IpfsMessaging::<Temporary>::new(None, account.clone(), None, None).await?)
                as Box<_>;
        Ok((account, raygun, did, identity))
    }

    #[tokio::test]
    async fn create_group_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, did_a, _) =
            create_account_and_chat(None, None, Some("test::create_group_conversation".into()))
                .await?;
        let (_account_b, mut chat_b, did_b, _) =
            create_account_and_chat(None, None, Some("test::create_group_conversation".into()))
                .await?;
        let (_account_c, mut chat_c, did_c, _) =
            create_account_and_chat(None, None, Some("test::create_group_conversation".into()))
                .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;
        let mut chat_subscribe_c = chat_c.subscribe().await?;

        chat_a
            .create_group_conversation(vec![did_b.clone(), did_c.clone()])
            .await?;

        let id_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_b.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_c = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        assert_eq!(id_a, id_b);
        assert_eq!(id_b, id_c);

        let conversation = chat_a.get_conversation(id_a).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Group);
        assert_eq!(conversation.recipients().len(), 3);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));
        Ok(())
    }

    #[tokio::test]
    async fn add_recipient_to_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, did_a, _) = create_account_and_chat(
            None,
            None,
            Some("test::add_recipient_to_conversation".into()),
        )
        .await?;
        let (_account_b, mut chat_b, did_b, _) = create_account_and_chat(
            None,
            None,
            Some("test::add_recipient_to_conversation".into()),
        )
        .await?;
        let (_account_c, mut chat_c, did_c, _) = create_account_and_chat(
            None,
            None,
            Some("test::add_recipient_to_conversation".into()),
        )
        .await?;
        let (_account_d, mut chat_d, did_d, _) = create_account_and_chat(
            None,
            None,
            Some("test::add_recipient_to_conversation".into()),
        )
        .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;
        let mut chat_subscribe_c = chat_c.subscribe().await?;
        let mut chat_subscribe_d = chat_d.subscribe().await?;

        chat_a
            .create_group_conversation(vec![did_b.clone(), did_c.clone()])
            .await?;

        let id_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_b.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_c = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_a = chat_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;
        let mut conversation_c = chat_c.get_conversation_stream(id_c).await?;

        chat_a.add_recipient(id_a, &did_d).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientAdded {
                    conversation_id,
                    recipient,
                }) = conversation_a.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_d);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientAdded {
                    conversation_id,
                    recipient,
                }) = conversation_b.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_d);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientAdded {
                    conversation_id,
                    recipient,
                }) = conversation_c.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_d);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_d.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    break;
                }
            }
        })
        .await?;

        let conversation = chat_a.get_conversation(id_a).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Group);
        assert_eq!(conversation.recipients().len(), 4);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));
        assert!(conversation.recipients().contains(&did_d));
        Ok(())
    }

    #[tokio::test]
    async fn remove_recipient_from_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, did_a, _) = create_account_and_chat(
            None,
            None,
            Some("test::remove_recipient_from_conversation".into()),
        )
        .await?;
        let (_account_b, mut chat_b, did_b, _) = create_account_and_chat(
            None,
            None,
            Some("test::remove_recipient_from_conversation".into()),
        )
        .await?;
        let (_account_c, mut chat_c, did_c, _) = create_account_and_chat(
            None,
            None,
            Some("test::remove_recipient_from_conversation".into()),
        )
        .await?;
        let (_account_d, mut chat_d, did_d, _) = create_account_and_chat(
            None,
            None,
            Some("test::remove_recipient_from_conversation".into()),
        )
        .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;
        let mut chat_subscribe_c = chat_c.subscribe().await?;
        let mut chat_subscribe_d = chat_d.subscribe().await?;

        chat_a
            .create_group_conversation(vec![did_b.clone(), did_c.clone()])
            .await?;

        let id_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_b.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_c = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_a = chat_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;
        let mut conversation_c = chat_c.get_conversation_stream(id_c).await?;

        chat_a.add_recipient(id_a, &did_d).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientAdded {
                    conversation_id,
                    recipient,
                }) = conversation_a.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_d);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientAdded {
                    conversation_id,
                    recipient,
                }) = conversation_b.next().await
                {
                    assert_eq!(conversation_id, id_b);
                    assert_eq!(recipient, did_d);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientAdded {
                    conversation_id,
                    recipient,
                }) = conversation_c.next().await
                {
                    assert_eq!(conversation_id, id_c);
                    assert_eq!(recipient, did_d);
                    break;
                }
            }
        })
        .await?;

        let id_d = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_d.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_d = chat_d.get_conversation_stream(id_d).await?;

        chat_a.remove_recipient(id_a, &did_b).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientRemoved {
                    conversation_id,
                    recipient,
                }) = conversation_a.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_b);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientRemoved {
                    conversation_id,
                    recipient,
                }) = conversation_c.next().await
                {
                    assert_eq!(conversation_id, id_c);
                    assert_eq!(recipient, did_b);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::RecipientRemoved {
                    conversation_id,
                    recipient,
                }) = conversation_d.next().await
                {
                    assert_eq!(conversation_id, id_c);
                    assert_eq!(recipient, did_b);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationDeleted { conversation_id }) =
                    chat_subscribe_b.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    break;
                }
            }
        })
        .await?;

        let conversation = chat_a.get_conversation(id_a).await?;

        assert_eq!(conversation.conversation_type(), ConversationType::Group);
        assert_eq!(conversation.recipients().len(), 3);
        assert!(conversation.recipients().contains(&did_a));
        assert!(!conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));
        assert!(conversation.recipients().contains(&did_d));
        Ok(())
    }

    #[tokio::test]
    async fn send_message_in_group_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, _, _) = create_account_and_chat(
            None,
            None,
            Some("test::send_message_in_group_conversation".into()),
        )
        .await?;
        let (_account_b, mut chat_b, did_b, _) = create_account_and_chat(
            None,
            None,
            Some("test::send_message_in_group_conversation".into()),
        )
        .await?;
        let (_account_c, mut chat_c, did_c, _) = create_account_and_chat(
            None,
            None,
            Some("test::send_message_in_group_conversation".into()),
        )
        .await?;
        let (_account_d, mut chat_d, did_d, _) = create_account_and_chat(
            None,
            None,
            Some("test::send_message_in_group_conversation".into()),
        )
        .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;
        let mut chat_subscribe_c = chat_c.subscribe().await?;
        let mut chat_subscribe_d = chat_d.subscribe().await?;

        chat_a
            .create_group_conversation(vec![did_b.clone(), did_c.clone(), did_d.clone()])
            .await?;

        let id_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_b.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_c = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_d = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_d.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_a = chat_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;
        let mut conversation_c = chat_c.get_conversation_stream(id_c).await?;
        let mut conversation_d = chat_c.get_conversation_stream(id_d).await?;


        chat_a.send(id_a, None, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break chat_a.get_message(conversation_id, message_id);
                }
            }.await
        })
        .await??;

        let message_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break chat_b.get_message(conversation_id, message_id);
                }
            }.await
        })
        .await??;

        let message_c = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_c.next().await
                {
                    break chat_c.get_message(conversation_id, message_id);
                }
            }.await
        })
        .await??;

        let message_d = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_d.next().await
                {
                    break chat_d.get_message(conversation_id, message_id);
                }
            }.await
        })
        .await??;


        assert_eq!(message_a, message_b);
        assert_eq!(message_b, message_c);
        assert_eq!(message_c, message_d);
        Ok(())
    }

}

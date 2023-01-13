#[cfg(test)]
mod test {
    use std::time::Duration;

    use futures::StreamExt;
    use warp::crypto::DID;
    use warp::multipass::identity::Identity;
    use warp::multipass::MultiPass;
    use warp::raygun::{MessageEventKind, PinState, RayGun, RayGunEventKind, ReactionState};
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
    async fn create_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, _did_a, _) =
            create_account_and_chat(None, None, Some("test::create_conversation".into())).await?;
        let (_account_b, mut chat_b, did_b, _) =
            create_account_and_chat(None, None, Some("test::create_conversation".into())).await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

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

        assert_eq!(id_a, id_b);
        Ok(())
    }

    #[tokio::test]
    async fn send_message_in_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, _did_a, _) = create_account_and_chat(
            None,
            None,
            Some("test::send_message_in_conversation".into()),
        )
        .await?;
        let (_account_b, mut chat_b, did_b, _) = create_account_and_chat(
            None,
            None,
            Some("test::send_message_in_conversation".into()),
        )
        .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

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

        let mut conversation_a = chat_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;

        chat_a.send(id_a, None, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break chat_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break chat_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);
        Ok(())
    }

    #[tokio::test]
    async fn delete_message_in_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, _did_a, _) = create_account_and_chat(
            None,
            None,
            Some("test::delete_message_in_conversation".into()),
        )
        .await?;
        let (_account_b, mut chat_b, did_b, _) = create_account_and_chat(
            None,
            None,
            Some("test::delete_message_in_conversation".into()),
        )
        .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

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

        let mut conversation_a = chat_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;

        chat_a.send(id_a, None, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break chat_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break chat_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);

        chat_a.delete(id_a, Some(message_a.id())).await?;

        // Maybe cross check the message id and convo
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageDeleted { .. }) = conversation_a.next().await {
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageDeleted { .. }) = conversation_b.next().await {
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn edit_message_in_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, _did_a, _) = create_account_and_chat(
            None,
            None,
            Some("test::edit_message_in_conversation".into()),
        )
        .await?;
        let (_account_b, mut chat_b, did_b, _) = create_account_and_chat(
            None,
            None,
            Some("test::edit_message_in_conversation".into()),
        )
        .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

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

        let mut conversation_a = chat_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;

        chat_a.send(id_a, None, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break chat_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break chat_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);

        chat_a
            .send(id_a, Some(message_a.id()), vec!["New Message".into()])
            .await?;

        let message_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageEdited {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break chat_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageEdited {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break chat_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);
        Ok(())
    }

    #[tokio::test]
    async fn react_message_in_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, did_a, _) = create_account_and_chat(
            None,
            None,
            Some("test::react_message_in_conversation".into()),
        )
        .await?;
        let (_account_b, mut chat_b, did_b, _) = create_account_and_chat(
            None,
            None,
            Some("test::react_message_in_conversation".into()),
        )
        .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

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

        let mut conversation_a = chat_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;

        chat_a.send(id_a, None, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break chat_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break chat_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);

        chat_a
            .react(id_a, message_a.id(), ReactionState::Add, ":smile:".into())
            .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReactionAdded {
                    conversation_id,
                    message_id,
                    did_key,
                    reaction,
                }) = conversation_a.next().await
                {
                    assert_eq!(id_a, conversation_id);
                    assert_eq!(message_id, message_a.id());
                    assert_eq!(did_key, did_a);
                    assert_eq!(reaction, ":smile:");
                    //Check the actual message itself
                    let message = chat_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    let reactions = message
                        .reactions()
                        .first()
                        .cloned()
                        .expect("Reaction exist");
                    assert!(reactions.users().contains(&did_a));
                    assert_eq!(reactions.emoji(), ":smile:");
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReactionAdded {
                    conversation_id,
                    message_id,
                    did_key,
                    reaction,
                }) = conversation_b.next().await
                {
                    assert_eq!(id_b, conversation_id);
                    assert_eq!(message_id, message_b.id());
                    assert_eq!(did_key, did_a);
                    assert_eq!(reaction, ":smile:");
                    //Check the actual message itself
                    let message = chat_b
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    let reactions = message
                        .reactions()
                        .first()
                        .cloned()
                        .expect("Reaction exist");
                    assert!(reactions.users().contains(&did_a));
                    assert_eq!(reactions.emoji(), ":smile:");
                    break;
                }
            }
        })
        .await?;

        chat_a
            .react(
                id_a,
                message_a.id(),
                ReactionState::Remove,
                ":smile:".into(),
            )
            .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReactionRemoved {
                    conversation_id,
                    message_id,
                    did_key,
                    reaction,
                }) = conversation_a.next().await
                {
                    assert_eq!(id_a, conversation_id);
                    assert_eq!(message_id, message_a.id());
                    assert_eq!(did_key, did_a);
                    assert_eq!(reaction, ":smile:");
                    //Check the actual message itself
                    let message = chat_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(message.reactions().is_empty());
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReactionRemoved {
                    conversation_id,
                    message_id,
                    did_key,
                    reaction,
                }) = conversation_b.next().await
                {
                    assert_eq!(id_b, conversation_id);
                    assert_eq!(message_id, message_b.id());
                    assert_eq!(did_key, did_a);
                    assert_eq!(reaction, ":smile:");
                    //Check the actual message itself
                    let message = chat_b
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(message.reactions().is_empty());
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn pin_message_in_conversation() -> anyhow::Result<()> {
        let (_account_a, mut chat_a, _did_a, _) =
            create_account_and_chat(None, None, Some("test::pin_message_in_conversation".into()))
                .await?;
        let (_account_b, mut chat_b, did_b, _) =
            create_account_and_chat(None, None, Some("test::pin_message_in_conversation".into()))
                .await?;

        let mut chat_subscribe_a = chat_a.subscribe().await?;
        let mut chat_subscribe_b = chat_b.subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

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

        let mut conversation_a = chat_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;

        chat_a.send(id_a, None, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break chat_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break chat_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);

        chat_a.pin(id_a, message_a.id(), PinState::Pin).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessagePinned {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    assert_eq!(id_a, conversation_id);
                    assert_eq!(message_id, message_a.id());
                    let message = chat_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(message.pinned());
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessagePinned {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    assert_eq!(id_b, conversation_id);
                    assert_eq!(message_id, message_b.id());
                    let message = chat_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(message.pinned());
                    break;
                }
            }
        })
        .await?;

        chat_a.pin(id_a, message_a.id(), PinState::Unpin).await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageUnpinned {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    assert_eq!(id_a, conversation_id);
                    assert_eq!(message_id, message_a.id());
                    let message = chat_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(!message.pinned());
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(MessageEventKind::MessageUnpinned {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    assert_eq!(id_b, conversation_id);
                    assert_eq!(message_id, message_b.id());
                    let message = chat_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(!message.pinned());
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }
}

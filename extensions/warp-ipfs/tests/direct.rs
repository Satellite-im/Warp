mod common;

#[cfg(test)]
mod test {
    use futures::{StreamExt, TryStreamExt};
    use std::time::Duration;
    use warp::{
        constellation::Progression,
        multipass::MultiPassEventKind,
        raygun::{
            AttachmentKind, ConversationType, Location, MessageEvent, MessageEventKind,
            MessageType, PinState, RayGunEventKind, ReactionState,
        },
    };

    use crate::common::{create_accounts_and_chat, PROFILE_IMAGE};

    #[tokio::test]
    async fn create_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts_and_chat(vec![
            (None, None, Some("test::create_conversation".into())),
            (None, None, Some("test::create_conversation".into())),
        ])
        .await?;

        let (_account_a, mut chat_a, _, did_a, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        let conversation = chat_a.get_conversation(id_a).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Direct);
        assert_eq!(conversation.recipients().len(), 2);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        Ok(())
    }

    #[tokio::test]
    async fn destroy_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts_and_chat(vec![
            (None, None, Some("test::destroy_conversation".into())),
            (None, None, Some("test::destroy_conversation".into())),
        ])
        .await?;

        let (_account_a, mut chat_a, _, did_a, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        let conversation = chat_a.get_conversation(id_a).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Direct);
        assert_eq!(conversation.recipients().len(), 2);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        let id = conversation.id();

        chat_a.delete(id, None).await?;

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationDeleted { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    assert_eq!(conversation_id, id);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationDeleted { conversation_id }) =
                    chat_subscribe_b.next().await
                {
                    assert_eq!(conversation_id, id);
                    break;
                }
            }
        })
        .await?;
        Ok(())
    }

    #[tokio::test]
    async fn send_message_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts_and_chat(vec![
            (
                None,
                None,
                Some("test::send_message_in_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::send_message_in_conversation".into()),
            ),
        ])
        .await?;

        let (_account_a, mut chat_a, _, _, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        chat_a.send(id_a, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(60), async {
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

        let message_b = tokio::time::timeout(Duration::from_secs(60), async {
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
    async fn send_and_download_attachment_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts_and_chat(vec![
            (
                None,
                None,
                Some("test::send_and_download_attachment_in_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::send_and_download_attachment_in_conversation".into()),
            ),
        ])
        .await?;

        let (_account_a, mut chat_a, mut fs_a, _, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        //upload file to constellation to attach file from constellation

        fs_a.put_buffer("image.png", PROFILE_IMAGE).await?;

        let (_, mut stream) = chat_a
            .attach(
                id_a,
                None,
                vec![Location::Constellation {
                    path: "image.png".into(),
                }],
                vec![],
            )
            .await?;

        while let Some(event) = stream.next().await {
            match event {
                AttachmentKind::AttachedProgress(
                    _location,
                    Progression::CurrentProgress { .. },
                ) => {}
                AttachmentKind::AttachedProgress(
                    _location,
                    Progression::ProgressComplete { name, total },
                ) => {
                    assert_eq!(name, "image.png");
                    assert_eq!(total, Some(PROFILE_IMAGE.len()));
                }
                AttachmentKind::AttachedProgress(_location, Progression::ProgressFailed { .. }) => {
                    unreachable!("should not fail")
                }
                AttachmentKind::Pending(result) => {
                    result?;
                }
            }
        }

        let message_a = tokio::time::timeout(Duration::from_secs(60), async {
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

        let message_b = tokio::time::timeout(Duration::from_secs(60), async {
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
        assert_eq!(message_a.message_type(), MessageType::Attachment);
        let attachments = message_a.attachments();
        assert!(!attachments.is_empty());

        let file = attachments.first().expect("attachment exist");

        assert_eq!(file.name(), "image.png");

        let stream = chat_b
            .download_stream(id_a, message_a.id(), "image.png")
            .await?;

        let data = stream.try_collect::<Vec<_>>().await?.concat();

        assert_eq!(data, PROFILE_IMAGE);
        Ok(())
    }

    #[tokio::test]
    async fn delete_message_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts_and_chat(vec![
            (
                None,
                None,
                Some("test::delete_message_in_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::delete_message_in_conversation".into()),
            ),
        ])
        .await?;

        let (_account_a, mut chat_a, _, _, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        chat_a.send(id_a, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(60), async {
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

        let message_b = tokio::time::timeout(Duration::from_secs(60), async {
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
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageDeleted { .. }) = conversation_a.next().await {
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(60), async {
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
        let accounts = create_accounts_and_chat(vec![
            (
                None,
                None,
                Some("test::edit_message_in_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::edit_message_in_conversation".into()),
            ),
        ])
        .await?;

        let (_account_a, mut chat_a, _, _, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        chat_a.send(id_a, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(60), async {
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

        let message_b = tokio::time::timeout(Duration::from_secs(60), async {
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
            .edit(id_a, message_a.id(), vec!["New Message".into()])
            .await?;

        let message_a = tokio::time::timeout(Duration::from_secs(60), async {
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

        let message_b = tokio::time::timeout(Duration::from_secs(60), async {
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
        let accounts = create_accounts_and_chat(vec![
            (
                None,
                None,
                Some("test::react_message_in_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::react_message_in_conversation".into()),
            ),
        ])
        .await?;

        let (_account_a, mut chat_a, _, did_a, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        chat_a.send(id_a, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(60), async {
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

        let message_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        tokio::time::timeout(Duration::from_secs(60), async {
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
                    let reactions = message.reactions();

                    let reactors = reactions.get(":smile:").expect("Reaction exist");
                    assert!(reactors.contains(&did_a));
                    assert!(reactions.contains_key(":smile:"));
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(60), async {
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
                    let reactions = message.reactions();

                    let reactors = reactions.get(":smile:").expect("Reaction exist");
                    assert!(reactors.contains(&did_a));
                    assert!(reactions.contains_key(":smile:"));
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

        tokio::time::timeout(Duration::from_secs(60), async {
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

        tokio::time::timeout(Duration::from_secs(60), async {
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
        let accounts = create_accounts_and_chat(vec![
            (None, None, Some("test::pin_message_in_conversation".into())),
            (None, None, Some("test::pin_message_in_conversation".into())),
        ])
        .await?;

        let (_account_a, mut chat_a, _, _, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        chat_a.send(id_a, vec!["Hello, World".into()]).await?;

        let message_a = tokio::time::timeout(Duration::from_secs(60), async {
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

        let message_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        tokio::time::timeout(Duration::from_secs(60), async {
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

        tokio::time::timeout(Duration::from_secs(60), async {
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

        tokio::time::timeout(Duration::from_secs(60), async {
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

        tokio::time::timeout(Duration::from_secs(60), async {
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

    #[tokio::test]
    async fn event_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts_and_chat(vec![
            (
                None,
                None,
                Some("test::event_message_in_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::event_message_in_conversation".into()),
            ),
        ])
        .await?;

        let (_account_a, mut chat_a, _, did_a, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_b.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_b = chat_b.get_conversation_stream(id_b).await?;

        chat_a.send_event(id_a, MessageEvent::Typing).await?;

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::EventReceived {
                    conversation_id,
                    did_key,
                    event,
                }) = conversation_b.next().await
                {
                    assert_eq!(conversation_id, id_b);
                    assert_eq!(did_key, did_a);
                    assert_eq!(event, MessageEvent::Typing);
                    break;
                }
            }
        })
        .await?;

        chat_a.cancel_event(id_a, MessageEvent::Typing).await?;

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::EventCancelled {
                    conversation_id,
                    did_key,
                    event,
                }) = conversation_b.next().await
                {
                    assert_eq!(conversation_id, id_b);
                    assert_eq!(did_key, did_a);
                    assert_eq!(event, MessageEvent::Typing);
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn delete_conversation_when_blocked() -> anyhow::Result<()> {
        let accounts = create_accounts_and_chat(vec![
            (
                None,
                None,
                Some("test::delete_conversation_when_blocked".into()),
            ),
            (
                None,
                None,
                Some("test::delete_conversation_when_blocked".into()),
            ),
        ])
        .await?;

        let (mut _account_a, mut chat_a, _, did_a, _) = accounts.first().cloned().unwrap();
        let (mut _account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut account_subscribe_a = _account_a.multipass_subscribe().await?;
        let mut account_subscribe_b = _account_b.multipass_subscribe().await?;

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = tokio::time::timeout(Duration::from_secs(60), async {
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

        let conversation = chat_a.get_conversation(id_a).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Direct);
        assert_eq!(conversation.recipients().len(), 2);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));

        _account_a.block(&did_b).await?;

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::Blocked { did }) = account_subscribe_a.next().await
                {
                    assert_eq!(did, did_b);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationDeleted { .. }) =
                    chat_subscribe_a.next().await
                {
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::BlockedBy { did }) =
                    account_subscribe_b.next().await
                {
                    assert_eq!(did, did_a);
                    break;
                }
            }
        })
        .await?;

        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationDeleted { .. }) =
                    chat_subscribe_b.next().await
                {
                    break;
                }
            }
        })
        .await?;
        Ok(())
    }
}

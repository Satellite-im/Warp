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

    use crate::common::{create_accounts, PROFILE_IMAGE};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;
    use warp::constellation::Constellation;
    use warp::multipass::{Friends, MultiPassEvent};
    use warp::raygun::{RayGun, RayGunAttachment, RayGunEvents, RayGunStream};

    #[async_test]
    async fn create_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (None, None, Some("test::create_conversation".into())),
            (None, None, Some("test::create_conversation".into())),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let conversation = instance_a.get_conversation(conversation_id).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Direct);
        assert_eq!(conversation.recipients().len(), 2);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        Ok(())
    }

    #[async_test]
    async fn conversation_favorite() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (None, None, Some("test::conversation_favorite".into())),
            (None, None, Some("test::conversation_favorite".into())),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let conversation = instance_a.get_conversation(conversation_id).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Direct);
        assert_eq!(conversation.recipients().len(), 2);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(!conversation.favorite());

        // favorite conversation
        instance_a
            .set_favorite_conversation(conversation_id, true)
            .await?;

        let conversation = instance_a.get_conversation(conversation_id).await?;
        assert!(conversation.favorite());

        // unmark conversation
        instance_a
            .set_favorite_conversation(conversation_id, false)
            .await?;

        let conversation = instance_a.get_conversation(conversation_id).await?;
        assert!(!conversation.favorite());
        Ok(())
    }

    #[async_test]
    async fn destroy_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (None, None, Some("test::destroy_conversation".into())),
            (None, None, Some("test::destroy_conversation".into())),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let conversation = instance_a.get_conversation(conversation_id).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Direct);
        assert_eq!(conversation.recipients().len(), 2);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        let id = conversation.id();

        instance_a.delete(id, None).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            let mut a_del = false;
            let mut b_del = false;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationDeleted { conversation_id }) = chat_subscribe_a.next() => {
                        assert_eq!(id, conversation_id);
                        a_del = true;
                    },
                    Some(RayGunEventKind::ConversationDeleted { conversation_id }) = chat_subscribe_b.next() => {
                        assert_eq!(id, conversation_id);
                        b_del = true;
                    },
                }

                if a_del && b_del {
                    break;
                }
            }
        }).await?;

        Ok(())
    }

    #[async_test]
    async fn send_message_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
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

        let (mut instance_a, _, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let mut conversation_a = instance_a.get_conversation_stream(conversation_id).await?;
        let mut conversation_b = instance_b.get_conversation_stream(conversation_id).await?;

        instance_a
            .send(conversation_id, vec!["Hello, World".into()])
            .await?;

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);
        Ok(())
    }

    #[async_test]
    async fn send_and_download_attachment_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
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

        let (mut instance_a, _, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let mut conversation_a = instance_a.get_conversation_stream(conversation_id).await?;
        let mut conversation_b = instance_b.get_conversation_stream(conversation_id).await?;

        //upload file to constellation to attach file from constellation

        instance_a.put_buffer("image.png", PROFILE_IMAGE).await?;

        let (_, mut stream) = instance_a
            .attach(
                conversation_id,
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

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_b.get_message(conversation_id, message_id).await;
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

        let stream = instance_b
            .download_stream(conversation_id, message_a.id(), "image.png")
            .await?;

        let data = stream.try_collect::<Vec<_>>().await?.concat();

        assert_eq!(data, PROFILE_IMAGE);
        Ok(())
    }

    #[async_test]
    async fn send_attachment_stream_and_download_attachment_in_conversation() -> anyhow::Result<()>
    {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::send_attachment_stream_and_download_attachment_in_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::send_attachment_stream_and_download_attachment_in_conversation".into()),
            ),
        ])
        .await?;

        let (mut instance_a, _, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let mut conversation_a = instance_a.get_conversation_stream(conversation_id).await?;
        let mut conversation_b = instance_b.get_conversation_stream(conversation_id).await?;

        let (_, mut stream) = instance_a
            .attach(
                conversation_id,
                None,
                vec![Location::Stream {
                    name: "image.png".into(),
                    stream: futures::stream::iter(vec![Ok(PROFILE_IMAGE.to_vec())]).boxed(),
                    size: Some(PROFILE_IMAGE.len()),
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

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_b.get_message(conversation_id, message_id).await;
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

        let stream = instance_b
            .download_stream(conversation_id, message_a.id(), "image.png")
            .await?;

        let data = stream.try_collect::<Vec<_>>().await?.concat();

        assert_eq!(data, PROFILE_IMAGE);
        Ok(())
    }

    #[async_test]
    async fn delete_message_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
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

        let (mut instance_a, _, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let mut conversation_a = instance_a.get_conversation_stream(conversation_id).await?;
        let mut conversation_b = instance_b.get_conversation_stream(conversation_id).await?;

        instance_a
            .send(conversation_id, vec!["Hello, World".into()])
            .await?;

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);

        instance_a
            .delete(conversation_id, Some(message_a.id()))
            .await?;

        // Maybe cross check the message id and convo
        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageDeleted { .. }) = conversation_a.next().await {
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageDeleted { .. }) = conversation_b.next().await {
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[async_test]
    async fn edit_message_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
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

        let (mut instance_a, _, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let mut conversation_a = instance_a.get_conversation_stream(conversation_id).await?;
        let mut conversation_b = instance_b.get_conversation_stream(conversation_id).await?;

        instance_a
            .send(conversation_id, vec!["Hello, World".into()])
            .await?;

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);

        instance_a
            .edit(conversation_id, message_a.id(), vec!["New Message".into()])
            .await?;

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageEdited {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageEdited {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);
        Ok(())
    }

    #[async_test]
    async fn react_message_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
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

        let (mut instance_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let mut conversation_a = instance_a.get_conversation_stream(conversation_id).await?;
        let mut conversation_b = instance_b.get_conversation_stream(conversation_id).await?;

        instance_a
            .send(conversation_id, vec!["Hello, World".into()])
            .await?;

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);

        instance_a
            .react(
                conversation_id,
                message_a.id(),
                ReactionState::Add,
                ":smile:".into(),
            )
            .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReactionAdded {
                    conversation_id,
                    message_id,
                    did_key,
                    reaction,
                }) = conversation_a.next().await
                {
                    assert_eq!(message_id, message_a.id());
                    assert_eq!(did_key, did_a);
                    assert_eq!(reaction, ":smile:");
                    //Check the actual message itself
                    let message = instance_a
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

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReactionAdded {
                    conversation_id,
                    message_id,
                    did_key,
                    reaction,
                }) = conversation_b.next().await
                {
                    assert_eq!(message_id, message_b.id());
                    assert_eq!(did_key, did_a);
                    assert_eq!(reaction, ":smile:");
                    //Check the actual message itself
                    let message = instance_b
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

        instance_a
            .react(
                conversation_id,
                message_a.id(),
                ReactionState::Remove,
                ":smile:".into(),
            )
            .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReactionRemoved {
                    conversation_id,
                    message_id,
                    did_key,
                    reaction,
                }) = conversation_a.next().await
                {
                    assert_eq!(message_id, message_a.id());
                    assert_eq!(did_key, did_a);
                    assert_eq!(reaction, ":smile:");
                    //Check the actual message itself
                    let message = instance_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(message.reactions().is_empty());
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReactionRemoved {
                    conversation_id,
                    message_id,
                    did_key,
                    reaction,
                }) = conversation_b.next().await
                {
                    assert_eq!(message_id, message_b.id());
                    assert_eq!(did_key, did_a);
                    assert_eq!(reaction, ":smile:");
                    //Check the actual message itself
                    let message = instance_b
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

    #[async_test]
    async fn change_conversation_description() -> anyhow::Result<()> {
        let accounts = create_accounts_and_chat(vec![
            (
                None,
                None,
                Some("test::change_conversation_description".into()),
            ),
            (
                None,
                None,
                Some("test::change_conversation_description".into()),
            ),
        ])
        .await?;

        let (_account_a, mut chat_a, _, _, _) = accounts.first().cloned().unwrap();
        let (_account_b, mut chat_b, _, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = chat_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = chat_b.raygun_subscribe().await?;

        chat_a.create_conversation(&did_b).await?;

        let id_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_b = crate::common::timeout(Duration::from_secs(60), async {
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

        chat_a
            .set_conversation_description(id_a, Some("hello, world!"))
            .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::ConversationDescriptionChanged {
                    conversation_id,
                    description,
                }) = conversation_a.next().await
                {
                    assert_eq!(id_a, conversation_id);
                    assert_eq!(description, Some("hello, world!".into()));
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::ConversationDescriptionChanged {
                    conversation_id,
                    description,
                }) = conversation_b.next().await
                {
                    assert_eq!(id_a, conversation_id);
                    assert_eq!(description, Some("hello, world!".into()));
                    break;
                }
            }
        })
        .await?;

        let conversation = chat_a.get_conversation(id_a).await?;
        assert_eq!(conversation.description().as_deref(), Some("hello, world!"));

        chat_a.set_conversation_description(id_a, None).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::ConversationDescriptionChanged {
                    conversation_id,
                    description,
                }) = conversation_a.next().await
                {
                    assert_eq!(id_a, conversation_id);
                    assert_eq!(description, None);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::ConversationDescriptionChanged {
                    conversation_id,
                    description,
                }) = conversation_b.next().await
                {
                    assert_eq!(id_a, conversation_id);
                    assert_eq!(description, None);
                    break;
                }
            }
        })
        .await?;

        let conversation = chat_a.get_conversation(id_a).await?;
        assert_eq!(conversation.description(), None);

        Ok(())
    }

    #[async_test]
    async fn pin_message_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (None, None, Some("test::pin_message_in_conversation".into())),
            (None, None, Some("test::pin_message_in_conversation".into())),
        ])
        .await?;

        let (mut instance_a, _, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let mut conversation_a = instance_a.get_conversation_stream(conversation_id).await?;
        let mut conversation_b = instance_b.get_conversation_stream(conversation_id).await?;

        instance_a
            .send(conversation_id, vec!["Hello, World".into()])
            .await?;

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_b.get_message(conversation_id, message_id).await;
                }
            }
        })
        .await??;

        assert_eq!(message_a, message_b);

        instance_a
            .pin(conversation_id, message_a.id(), PinState::Pin)
            .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessagePinned {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    assert_eq!(message_id, message_a.id());
                    let message = instance_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(message.pinned());
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessagePinned {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    assert_eq!(message_id, message_b.id());
                    let message = instance_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(message.pinned());
                    break;
                }
            }
        })
        .await?;

        instance_a
            .pin(conversation_id, message_a.id(), PinState::Unpin)
            .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageUnpinned {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    assert_eq!(message_id, message_a.id());
                    let message = instance_a
                        .get_message(conversation_id, message_id)
                        .await
                        .expect("Message exist");
                    assert!(!message.pinned());
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageUnpinned {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    assert_eq!(message_id, message_b.id());
                    let message = instance_a
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

    #[async_test]
    async fn event_in_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
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

        let (mut instance_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let mut conversation_b = instance_b.get_conversation_stream(conversation_id).await?;

        instance_a
            .send_event(conversation_id, MessageEvent::Typing)
            .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::EventReceived {
                    conversation_id: _,
                    did_key,
                    event,
                }) = conversation_b.next().await
                {
                    assert_eq!(did_key, did_a);
                    assert_eq!(event, MessageEvent::Typing);
                    break;
                }
            }
        })
        .await?;

        instance_a
            .cancel_event(conversation_id, MessageEvent::Typing)
            .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::EventCancelled {
                    conversation_id: _,
                    did_key,
                    event,
                }) = conversation_b.next().await
                {
                    assert_eq!(did_key, did_a);
                    assert_eq!(event, MessageEvent::Typing);
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[async_test]
    async fn delete_conversation_when_blocked() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
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

        let (mut instance_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut account_subscribe_a = instance_a.multipass_subscribe().await?;
        let mut account_subscribe_b = instance_b.multipass_subscribe().await?;

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a.create_conversation(&did_b).await?;

        let conversation_id = crate::common::timeout(Duration::from_secs(60), async {
            let mut id_a = None;
            let mut id_b = None;
            loop {
                tokio::select! {
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_a.next() => {
                        id_a.replace(conversation_id);
                    },
                    Some(RayGunEventKind::ConversationCreated { conversation_id }) = chat_subscribe_b.next() => {
                        id_b.replace(conversation_id);
                    },
                }

                if id_a.is_some() && id_b.is_some() {
                    assert_eq!(id_a, id_b);
                    break id_a.expect("valid conversation_id")
                }
            }
        }).await?;

        let conversation = instance_a.get_conversation(conversation_id).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Direct);
        assert_eq!(conversation.recipients().len(), 2);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));

        instance_a.block(&did_b).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::Blocked { did }) = account_subscribe_a.next().await
                {
                    assert_eq!(did, did_b);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationDeleted { .. }) =
                    chat_subscribe_a.next().await
                {
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

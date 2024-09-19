mod common;
#[cfg(test)]
mod test {
    use std::{collections::HashMap, time::Duration};

    use crate::common::create_accounts;
    use futures::StreamExt;
    use warp::{
        multipass::MultiPassEventKind,
        raygun::{
            ConversationType, GroupPermission, MessageEventKind, RayGunEventKind
        },
    };

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;

    use warp::multipass::{Friends, MultiPassEvent};
    use warp::raygun::{RayGun, RayGunGroupConversation, RayGunStream};

    use uuid::Uuid;
    use warp::error::Error;

    #[async_test]
    async fn create_empty_group_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![(
            None,
            None,
            Some("test::create_empty_group_conversation".into()),
        )])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(None, vec![], HashMap::new())
            .await?;

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

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(
            conversation.permissions(),
            HashMap::new(),
        );
        assert_eq!(conversation.recipients().len(), 1);
        assert!(conversation.recipients().contains(&did_a));

        Ok(())
    }

    #[async_test]
    async fn update_group_conversation_name() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![(
            None,
            None,
            Some("test::update_group_conversation_name".into()),
        )])
        .await?;

        let (mut instance_a, _, _) = accounts[0].clone();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(None, vec![], HashMap::new())
            .await?;

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

        let mut conversation_a = instance_a.get_conversation_stream(id_a).await?;

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(conversation.name(), None);

        instance_a.update_conversation_name(id_a, "test").await?;

        let name = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::ConversationNameUpdated { name, .. }) =
                    conversation_a.next().await
                {
                    break name;
                }
            }
        })
        .await?;

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(conversation.name(), Some(name));

        instance_a.update_conversation_name(id_a, "").await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::ConversationNameUpdated { .. }) =
                    conversation_a.next().await
                {
                    break;
                }
            }
        })
        .await?;

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(conversation.name(), None);

        Ok(())
    }

    #[async_test]
    async fn create_group_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (None, None, Some("test::create_group_conversation".into())),
            (None, None, Some("test::create_group_conversation".into())),
            (None, None, Some("test::create_group_conversation".into())),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(
                None,
                vec![did_b.clone(), did_c.clone()],
                HashMap::new(),
            )
            .await?;

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

        let id_c = crate::common::timeout(Duration::from_secs(60), async {
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

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(
            conversation.permissions(),
            HashMap::new(),
        );
        assert_eq!(conversation.recipients().len(), 3);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));
        Ok(())
    }

    #[async_test]
    async fn destroy_group_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (None, None, Some("test::destroy_group_conversation".into())),
            (None, None, Some("test::destroy_group_conversation".into())),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts.first().cloned().unwrap();
        let (mut instance_b, did_b, _) = accounts.last().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(None, vec![did_b.clone()], Default::default())
            .await?;

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

        assert_eq!(id_a, id_b);

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(conversation.conversation_type(), ConversationType::Group);
        assert_eq!(conversation.recipients().len(), 2);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        let id = conversation.id();

        instance_a.delete(id, None).await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

    #[async_test]
    async fn member_change_name_to_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::member_change_name_to_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::member_change_name_to_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::member_change_name_to_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::member_change_name_to_conversation".into()),
            ),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();
        let (mut instance_d, did_d, _) = accounts[3].clone();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;
        let mut chat_subscribe_d = instance_d.raygun_subscribe().await?;

        let mut permissions = HashMap::new();
        permissions.insert(did_b.clone(), vec![GroupPermission::SetGroupName]);
        permissions.insert(did_c.clone(), vec![GroupPermission::SetGroupName]);

        instance_a
            .create_group_conversation(None, vec![did_b.clone(), did_c.clone()], permissions.clone())
            .await?;

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

        let id_c = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_a = instance_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = instance_b.get_conversation_stream(id_b).await?;
        let mut conversation_c = instance_c.get_conversation_stream(id_c).await?;

        instance_a.add_recipient(id_a, &did_d).await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        instance_b.update_conversation_name(id_a, "test").await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::ConversationNameUpdated { name, .. }) =
                    conversation_a.next().await
                {
                    assert_eq!(name, "test");
                    break;
                }
            }
        })
        .await?;

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(
            conversation.permissions(),
            permissions,
        );
        assert_eq!(conversation.name().as_deref(), Some("test"));
        assert_eq!(conversation.recipients().len(), 4);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));
        assert!(conversation.recipients().contains(&did_d));
        Ok(())
    }

    #[async_test]
    async fn add_recipient_to_conversation_to_open_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::add_recipient_to_conversation_to_open_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::add_recipient_to_conversation_to_open_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::add_recipient_to_conversation_to_open_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::add_recipient_to_conversation_to_open_conversation".into()),
            ),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();
        let (mut instance_d, did_d, _) = accounts[3].clone();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;
        let mut chat_subscribe_d = instance_d.raygun_subscribe().await?;

        let mut permissions = HashMap::new();
        permissions.insert(did_b.clone(), vec![GroupPermission::AddParticipants]);
        permissions.insert(did_c.clone(), vec![GroupPermission::AddParticipants]);

        instance_a
            .create_group_conversation(None, vec![did_b.clone(), did_c.clone()], permissions.clone())
            .await?;

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

        let id_c = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_a = instance_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = instance_b.get_conversation_stream(id_b).await?;
        let mut conversation_c = instance_c.get_conversation_stream(id_c).await?;

        instance_b.add_recipient(id_b, &did_d).await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        let ret = instance_b.update_conversation_name(id_b, "test").await;

        if permissions[&did_b].contains(&GroupPermission::SetGroupName) && permissions[&did_c].contains(&GroupPermission::SetGroupName) {
            // Non-owner should be able to change the name.
            ret?;
        } else {
            // First attempt to change the name as a non-onwer should fail since the
            // settings don't allow it.
            assert!(ret.is_err());
            // Second attempt to change the name as an owner should work.
            instance_a.update_conversation_name(id_a, "test").await?;
        }

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(
            conversation.permissions(),
            permissions,
        );
        assert_eq!(conversation.recipients().len(), 4);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));
        assert!(conversation.recipients().contains(&did_d));
        Ok(())
    }

    #[async_test]
    async fn add_recipient_to_conversation_to_close_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::add_recipient_to_conversation_to_close_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::add_recipient_to_conversation_to_close_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::add_recipient_to_conversation_to_close_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::add_recipient_to_conversation_to_close_conversation".into()),
            ),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();
        let (mut instance_d, did_d, _) = accounts[3].clone();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;
        let mut chat_subscribe_d = instance_d.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(
                None,
                vec![did_b.clone(), did_c.clone()],
                HashMap::new(),
            )
            .await?;

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

        let id_c = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_a = instance_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = instance_b.get_conversation_stream(id_b).await?;
        let mut conversation_c = instance_c.get_conversation_stream(id_c).await?;

        instance_a.add_recipient(id_a, &did_d).await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(
            conversation.permissions(),
            HashMap::new(),
        );
        assert_eq!(conversation.recipients().len(), 4);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));
        assert!(conversation.recipients().contains(&did_d));
        Ok(())
    }

    #[async_test]
    async fn attempt_to_add_blocked_recipient() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::attempt_to_add_blocked_recipient".into()),
            ),
            (
                None,
                None,
                Some("test::attempt_to_add_blocked_recipient".into()),
            ),
            (
                None,
                None,
                Some("test::attempt_to_add_blocked_recipient".into()),
            ),
            (
                None,
                None,
                Some("test::attempt_to_add_blocked_recipient".into()),
            ),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();
        let (mut instance_d, did_d, _) = accounts[3].clone();

        let mut subscribe_a = instance_a.multipass_subscribe().await?;
        let mut subscribe_d = instance_d.multipass_subscribe().await?;

        instance_a.block(&did_d).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::Blocked { did }) = subscribe_a.next().await {
                    assert_eq!(did, did_d);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::BlockedBy { did }) = subscribe_d.next().await {
                    assert_eq!(did, did_a);
                    break;
                }
            }
        })
        .await?;

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;

        let mut permissions = HashMap::new();
        permissions.insert(did_b.clone(), vec![GroupPermission::AddParticipants]);
        permissions.insert(did_c.clone(), vec![GroupPermission::AddParticipants]);

        instance_a
            .create_group_conversation(None, vec![did_b.clone(), did_c.clone()], permissions)
            .await?;

        let id = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_a.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_b.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        instance_b
            .add_recipient(id, &did_d)
            .await
            .expect_err("identity is blocked");

        Ok(())
    }

    #[async_test]
    async fn remove_recipient_from_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::remove_recipient_from_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::remove_recipient_from_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::remove_recipient_from_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::remove_recipient_from_conversation".into()),
            ),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();
        let (mut instance_d, did_d, _) = accounts[3].clone();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;
        let mut chat_subscribe_d = instance_d.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(
                None,
                vec![did_b.clone(), did_c.clone()],
                HashMap::new(),
            )
            .await?;

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

        let id_c = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_a = instance_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = instance_b.get_conversation_stream(id_b).await?;
        let mut conversation_c = instance_c.get_conversation_stream(id_c).await?;

        instance_a.add_recipient(id_a, &did_d).await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        let id_d = crate::common::timeout(Duration::from_secs(60), async {
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

        let mut conversation_d = instance_d.get_conversation_stream(id_d).await?;

        instance_a.remove_recipient(id_a, &did_b).await?;

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        crate::common::timeout(Duration::from_secs(60), async {
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

        let conversation = instance_a.get_conversation(id_a).await?;

        assert_eq!(
            conversation.permissions(),
            HashMap::new(),
        );
        assert_eq!(conversation.recipients().len(), 3);
        assert!(conversation.recipients().contains(&did_a));
        assert!(!conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));
        assert!(conversation.recipients().contains(&did_d));
        Ok(())
    }

    #[async_test]
    async fn send_message_in_group_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::send_message_in_group_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::send_message_in_group_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::send_message_in_group_conversation".into()),
            ),
            (
                None,
                None,
                Some("test::send_message_in_group_conversation".into()),
            ),
        ])
        .await?;

        let (mut instance_a, _, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();
        let (mut instance_d, did_d, _) = accounts[3].clone();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;
        let mut chat_subscribe_d = instance_d.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(
                None,
                vec![did_b.clone(), did_c.clone(), did_d.clone()],
                HashMap::new(),
            )
            .await?;

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

        let id_c = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_c.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let id_d = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(RayGunEventKind::ConversationCreated { conversation_id }) =
                    chat_subscribe_d.next().await
                {
                    break conversation_id;
                }
            }
        })
        .await?;

        let mut conversation_a = instance_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = instance_b.get_conversation_stream(id_b).await?;
        let mut conversation_c = instance_c.get_conversation_stream(id_c).await?;
        let mut conversation_d = instance_d.get_conversation_stream(id_d).await?;

        instance_a.send(id_a, vec!["Hello, World".into()]).await?;

        let message_a = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageSent {
                    conversation_id,
                    message_id,
                }) = conversation_a.next().await
                {
                    break instance_a.get_message(conversation_id, message_id);
                }
            }
            .await
        })
        .await??;

        let message_b = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_b.next().await
                {
                    break instance_b.get_message(conversation_id, message_id);
                }
            }
            .await
        })
        .await??;

        let message_c = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_c.next().await
                {
                    break instance_c.get_message(conversation_id, message_id);
                }
            }
            .await
        })
        .await??;

        let message_d = crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::MessageReceived {
                    conversation_id,
                    message_id,
                }) = conversation_d.next().await
                {
                    break instance_d.get_message(conversation_id, message_id);
                }
            }
            .await
        })
        .await??;

        assert_eq!(message_a, message_b);
        assert_eq!(message_b, message_c);
        assert_eq!(message_c, message_d);
        Ok(())
    }

    #[async_test]
    async fn remove_recipient_from_conversation_when_blocked() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::remove_recipient_from_conversation_when_blocked".into()),
            ),
            (
                None,
                None,
                Some("test::remove_recipient_from_conversation_when_blocked".into()),
            ),
            (
                None,
                None,
                Some("test::remove_recipient_from_conversation_when_blocked".into()),
            ),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();

        let mut account_subscribe_a = instance_a.multipass_subscribe().await?;
        let mut account_subscribe_c = instance_c.multipass_subscribe().await?;

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(
                None,
                vec![did_b.clone(), did_c.clone()],
                HashMap::new(),
            )
            .await?;

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

        let id_c = crate::common::timeout(Duration::from_secs(60), async {
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

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(
            conversation.permissions(),
            HashMap::new(),
        );
        assert_eq!(conversation.recipients().len(), 3);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));

        let mut conversation_a = instance_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = instance_b.get_conversation_stream(id_b).await?;

        instance_a.block(&did_c).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::Blocked { did }) = account_subscribe_a.next().await
                {
                    assert_eq!(did, did_c);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::RecipientRemoved {
                    conversation_id,
                    recipient,
                }) = conversation_a.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_c);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::RecipientRemoved {
                    conversation_id,
                    recipient,
                }) = conversation_b.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_c);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::BlockedBy { did }) =
                    account_subscribe_c.next().await
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
                    chat_subscribe_c.next().await
                {
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[async_test]
    async fn delete_group_conversation_when_blocking_creator() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![
            (
                None,
                None,
                Some("test::delete_group_conversation_when_blocking_creator".into()),
            ),
            (
                None,
                None,
                Some("test::delete_group_conversation_when_blocking_creator".into()),
            ),
            (
                None,
                None,
                Some("test::delete_group_conversation_when_blocking_creator".into()),
            ),
        ])
        .await?;

        let (mut instance_a, did_a, _) = accounts[0].clone();
        let (mut instance_b, did_b, _) = accounts[1].clone();
        let (mut instance_c, did_c, _) = accounts[2].clone();

        let mut account_subscribe_a = instance_a.multipass_subscribe().await?;
        let mut account_subscribe_c = instance_c.multipass_subscribe().await?;

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;
        let mut chat_subscribe_b = instance_b.raygun_subscribe().await?;
        let mut chat_subscribe_c = instance_c.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(
                None,
                vec![did_b.clone(), did_c.clone()],
                HashMap::new(),
            )
            .await?;

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

        let id_c = crate::common::timeout(Duration::from_secs(60), async {
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

        let conversation = instance_a.get_conversation(id_a).await?;
        assert_eq!(
            conversation.permissions(),
            HashMap::new(),
        );
        assert_eq!(conversation.recipients().len(), 3);
        assert!(conversation.recipients().contains(&did_a));
        assert!(conversation.recipients().contains(&did_b));
        assert!(conversation.recipients().contains(&did_c));

        let mut conversation_a = instance_a.get_conversation_stream(id_a).await?;
        let mut conversation_b = instance_b.get_conversation_stream(id_b).await?;

        instance_c.block(&did_a).await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::BlockedBy { did }) =
                    account_subscribe_a.next().await
                {
                    assert_eq!(did, did_c);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::RecipientRemoved {
                    conversation_id,
                    recipient,
                }) = conversation_a.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_c);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MessageEventKind::RecipientRemoved {
                    conversation_id,
                    recipient,
                }) = conversation_b.next().await
                {
                    assert_eq!(conversation_id, id_a);
                    assert_eq!(recipient, did_c);
                    break;
                }
            }
        })
        .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            loop {
                if let Some(MultiPassEventKind::Blocked { did }) = account_subscribe_c.next().await
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
                    chat_subscribe_c.next().await
                {
                    break;
                }
            }
        })
        .await?;

        Ok(())
    }

    #[async_test]
    async fn archive_unarchive_conversation() -> anyhow::Result<()> {
        let accounts = create_accounts(vec![(
            None,
            None,
            Some("test::archive_unarchive_conversation".into()),
        )])
        .await?;

        let (mut instance_a, _, _) = accounts.first().cloned().unwrap();

        let mut chat_subscribe_a = instance_a.raygun_subscribe().await?;

        instance_a
            .create_group_conversation(None, vec![], Default::default())
            .await?;

        crate::common::timeout(Duration::from_secs(60), async {
            let mut c_id = Uuid::nil();
            let mut archived = false;
            let mut unarchived = false;
            loop {
                tokio::select! {
                    Some(event) = chat_subscribe_a.next() => {
                        match event {
                            RayGunEventKind::ConversationCreated{ conversation_id } => {
                                c_id = conversation_id;
                                instance_a.archived_conversation(conversation_id).await?;
                            }
                            RayGunEventKind::ConversationArchived{ conversation_id } => {
                                assert_eq!(conversation_id, c_id);
                                assert!(!archived);
                                assert!(!unarchived);
                                let conversation = instance_a.get_conversation(c_id).await?;
                                assert!(conversation.archived());
                                archived = true;
                                instance_a.unarchived_conversation(conversation_id).await?;
                            }
                            RayGunEventKind::ConversationUnarchived{ conversation_id } => {
                                assert_eq!(conversation_id, c_id);
                                assert!(archived);
                                assert!(!unarchived);
                                let conversation = instance_a.get_conversation(conversation_id).await?;
                                assert!(!conversation.archived());
                                unarchived = true;
                            }
                            RayGunEventKind::ConversationDeleted { .. } => unreachable!()
                        }
                    }
                }
                if c_id != Uuid::nil() && archived && unarchived {
                    break;
                }
            }
            Ok::<_, Error>(())
        })
        .await??;

        Ok(())
    }
}

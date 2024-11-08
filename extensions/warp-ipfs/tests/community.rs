mod common;
#[cfg(test)]
mod test {
    use futures::StreamExt;
    use std::time::Duration;
    use warp::raygun::{
        community::{
            Community, CommunityChannelPermission, CommunityChannelType, CommunityInvite,
            CommunityPermission, RayGunCommunity,
        },
        MessageEventKind, MessageEventStream,
    };

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;

    use warp::error::Error;

    use crate::common::create_accounts;

    async fn next_event(
        stream: &mut MessageEventStream,
        timeout: Duration,
    ) -> Result<MessageEventKind, std::io::Error> {
        crate::common::timeout(timeout, async {
            loop {
                if let Some(event) = stream.next().await {
                    return event;
                }
            }
        })
        .await
    }
    #[async_test]
    async fn get_community_stream() -> anyhow::Result<()> {
        let context = Some("test::get_community_stream".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![
            account_opts.clone(),
            account_opts.clone(),
            account_opts,
        ])
        .await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();
        let (instance_c, did_c, _) = &mut accounts[2].clone();

        let community = instance_a.create_community("Community0").await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::CreatedCommunityInvite {
                community_id: community.id(),
                invite: invite.clone()
            }
        );

        let invite_list = instance_b.list_communities_invited_to().await?;
        assert!(invite_list.len() == 1);
        let mut stream_b = instance_b.get_community_stream(community.id()).await?;
        for (community_id, invite) in invite_list {
            instance_b
                .accept_community_invite(community_id, invite.id())
                .await?;
        }
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );
        assert_eq!(
            next_event(&mut stream_b, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_c.clone()), None)
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::CreatedCommunityInvite {
                community_id: community.id(),
                invite: invite.clone()
            }
        );
        assert_eq!(
            next_event(&mut stream_b, Duration::from_secs(60)).await?,
            MessageEventKind::CreatedCommunityInvite {
                community_id: community.id(),
                invite: invite.clone()
            }
        );

        let invite_list = instance_c.list_communities_invited_to().await?;
        assert!(invite_list.len() == 1);
        for (community_id, invite) in invite_list {
            instance_c
                .accept_community_invite(community_id, invite.id())
                .await?;
        }
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_c.clone()
            }
        );
        assert_eq!(
            next_event(&mut stream_b, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_c.clone()
            }
        );

        let community_seen_by_a = instance_a.get_community(community.id()).await?;
        let community_seen_by_b = instance_b.get_community(community.id()).await?;
        let community_seen_by_c = instance_c.get_community(community.id()).await?;

        assert_eq!(
            format!("{:?}", community_seen_by_a),
            format!("{:?}", community_seen_by_b)
        );
        assert_eq!(
            format!("{:?}", community_seen_by_b),
            format!("{:?}", community_seen_by_c)
        );
        Ok(())
    }

    // #[async_test]
    // async fn delete_community_as_creator() -> anyhow::Result<()> {
    //     let context = Some("test::delete_community_as_creator".into());
    //     let account_opts = (None, None, context);
    //     let mut accounts = create_accounts(vec![account_opts]).await?;

    //     let (instance_a, _, _) = &mut accounts[0];
    //     let community = instance_a.create_community("Community0").await?;
    //     instance_a.delete_community(community.id()).await?;
    //     Ok(())
    // }
    // #[async_test]
    // async fn delete_community_as_non_creator() -> anyhow::Result<()> {
    //     let context = Some("test::delete_community_as_non_creator".into());
    //     let account_opts = (None, None, context);
    //     let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;

    //     let (instance_a, _, _) = &mut accounts[0];
    //     let community = instance_a.create_community("Community0").await?;

    //     let (instance_b, _, _) = &mut accounts[1];
    //     match instance_b.delete_community(community.id()).await {
    //         Err(e) => match e {
    //             Error::Unauthorized => {}
    //             _ => panic!("error should be Error::Unauthorized"),
    //         },
    //         Ok(_) => panic!("should be unauthorized to delete community"),
    //     }
    //     Ok(())
    // }

    #[async_test]
    async fn get_community_as_uninvited_user() -> anyhow::Result<()> {
        let context = Some("test::get_community_as_uninvited_user".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, _, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;

        let result = instance_b.get_community(community.id()).await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::InvalidCommunity))
        );
        Ok(())
    }
    #[async_test]
    async fn get_community_as_invited_user() -> anyhow::Result<()> {
        let context = Some("test::get_community_as_invited_user".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(
                community.id(),
                Some(did_b.clone()),
                Some(chrono::Utc::now() + chrono::Duration::days(1)),
            )
            .await?;

        let got_community = instance_b.get_community(community.id()).await?;
        assert!(got_community.invites().contains(&invite.id()));
        Ok(())
    }
    #[async_test]
    async fn get_community_as_member() -> anyhow::Result<()> {
        let context = Some("test::get_community_as_member".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;

        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;

        let got_community = instance_b.get_community(community.id()).await?;
        assert!(got_community.members().contains(&did_b.clone()));
        Ok(())
    }

    #[async_test]
    async fn list_community_joined() -> anyhow::Result<()> {
        let context = Some("test::list_communites_joined".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;

        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;

        let list = instance_b.list_communities_joined().await?;
        assert!(list.len() == 1);
        assert!(list[0] == community.id());
        Ok(())
    }

    #[async_test]
    async fn list_community_invited_to() -> anyhow::Result<()> {
        let context = Some("test::list_communites_invited_to".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;

        let list = instance_b.list_communities_invited_to().await?;
        assert!(list.len() == 1);
        let (community_id, invited_to) = &list[0];
        assert!(community_id == &community.id());
        assert!(invited_to == &invite);
        Ok(())
    }

    // #[async_test]
    // async fn leave_community() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn unauthorized_edit_community_icon() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn authorized_edit_community_icon() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn unauthorized_edit_community_banner() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn authorized_edit_community_banner() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }

    #[async_test]
    async fn unauthorized_create_community_invite() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_create_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;

        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .create_community_invite(community.id(), None, None)
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<CommunityInvite, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_create_community_invite() -> anyhow::Result<()> {
        let context = Some("test::authorized_create_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![
            account_opts.clone(),
            account_opts.clone(),
            account_opts,
        ])
        .await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();
        let (instance_c, did_c, _) = &mut accounts[2].clone();

        let community = instance_a.create_community("Community0").await?;
        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManageInvites,
                role.id(),
            )
            .await?;

        let invite_for_b = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;

        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite_for_b.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite_for_b.id(),
                user: did_b.clone()
            }
        );

        let mut stream_b = instance_b.get_community_stream(community.id()).await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        assert_eq!(
            next_event(&mut stream_b, Duration::from_secs(60)).await?,
            MessageEventKind::GrantedCommunityRole {
                community_id: community.id(),
                role_id: role.id(),
                user: did_b.clone()
            }
        );
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::GrantedCommunityRole {
                community_id: community.id(),
                role_id: role.id(),
                user: did_b.clone()
            }
        );

        let invite_for_c = instance_b
            .create_community_invite(community.id(), Some(did_c.clone()), None)
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::CreatedCommunityInvite {
                community_id: community.id(),
                invite: invite_for_c.clone(),
            }
        );

        instance_c
            .accept_community_invite(community.id(), invite_for_c.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite_for_c.id(),
                user: did_c.clone()
            }
        );
        Ok(())
    }
    #[async_test]
    async fn unauthorized_delete_community_invite() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_delete_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite_for_b = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;

        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite_for_b.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite_for_b.id(),
                user: did_b.clone()
            }
        );

        let invite_to_try_delete = instance_a
            .create_community_invite(community.id(), None, None)
            .await?;

        let result = instance_b
            .delete_community_invite(community.id(), invite_to_try_delete.id())
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_delete_community_invite() -> anyhow::Result<()> {
        let context = Some("test::authorized_delete_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;

        let invite_for_b = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite_for_b.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite_for_b.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManageInvites,
                role.id(),
            )
            .await?;

        let invite_to_delete = instance_a
            .create_community_invite(community.id(), None, None)
            .await?;
        instance_b
            .delete_community_invite(community.id(), invite_to_delete.id())
            .await?;

        let community = instance_a.get_community(community.id()).await?;
        assert!(!community.invites().contains(&invite_to_delete.id()));
        Ok(())
    }
    #[async_test]
    async fn get_community_invite() -> anyhow::Result<()> {
        let context = Some("test::get_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;

        let got_invite = instance_b
            .get_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(invite, got_invite);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_invite() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite_for_b = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite_for_b.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite_for_b.id(),
                user: did_b.clone()
            }
        );

        let mut invite = instance_a
            .create_community_invite(community.id(), None, None)
            .await?;
        invite.set_target_user(Some(did_b.clone()));
        let result = instance_b
            .edit_community_invite(community.id(), invite.id(), invite)
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_invite() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite_for_b = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite_for_b.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite_for_b.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManageInvites,
                role.id(),
            )
            .await?;

        let mut invite = instance_a
            .create_community_invite(community.id(), None, None)
            .await?;
        invite.set_target_user(Some(did_b.clone()));
        instance_b
            .edit_community_invite(community.id(), invite.id(), invite.clone())
            .await?;

        let got_invite = instance_a
            .get_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(invite, got_invite);
        Ok(())
    }
    #[async_test]
    async fn accept_valid_community_invite() -> anyhow::Result<()> {
        let context = Some("test::accept_valid_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite_for_b = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite_for_b.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite_for_b.id(),
                user: did_b.clone()
            }
        );
        Ok(())
    }
    #[async_test]
    async fn try_accept_wrong_target_community_invite() -> anyhow::Result<()> {
        let context = Some("test::try_accept_wrong_target_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![
            account_opts.clone(),
            account_opts.clone(),
            account_opts,
        ])
        .await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, _, _) = &mut accounts[1].clone();
        let (_, did_c, _) = &mut accounts[2].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite_for_b = instance_a
            .create_community_invite(community.id(), Some(did_c.clone()), None)
            .await?;
        let result = instance_b
            .accept_community_invite(community.id(), invite_for_b.id())
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::InvalidCommunity))
        );
        Ok(())
    }
    #[async_test]
    async fn try_accept_expired_community_invite() -> anyhow::Result<()> {
        let context = Some("test::try_accept_expired_community_invite".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite_for_b = instance_a
            .create_community_invite(
                community.id(),
                Some(did_b.clone()),
                Some(chrono::Utc::now() - chrono::Duration::days(1)),
            )
            .await?;
        let result = instance_b
            .accept_community_invite(community.id(), invite_for_b.id())
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!(
                "{:?}",
                Err::<Community, Error>(Error::CommunityInviteExpired)
            )
        );
        Ok(())
    }

    #[async_test]
    async fn unauthorized_get_community_channel() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_get_community_channel".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;
        instance_a
            .revoke_community_channel_permission_for_all(
                community.id(),
                channel.id(),
                CommunityChannelPermission::ViewChannel,
            )
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .get_community_channel(community.id(), channel.id())
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_get_community_channel() -> anyhow::Result<()> {
        let context = Some("test::authorized_get_community_channel".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let got_channel = instance_b
            .get_community_channel(community.id(), channel.id())
            .await?;

        assert_eq!(channel, got_channel);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_create_community_channel() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_create_community_channel".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_create_community_channel() -> anyhow::Result<()> {
        let context = Some("test::authorized_create_community_channel".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManageChannels,
                role.id(),
            )
            .await?;

        let channel = instance_b
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;

        let got_channel = instance_a
            .get_community_channel(community.id(), channel.id())
            .await?;

        assert_eq!(channel, got_channel);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_delete_community_channel() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_delete_community_channel".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;
        let result = instance_b
            .delete_community_channel(community.id(), channel.id())
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_delete_community_channel() -> anyhow::Result<()> {
        let context = Some("test::authorized_delete_community_channel".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManageChannels,
                role.id(),
            )
            .await?;

        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;
        let community = instance_a.get_community(community.id()).await?;
        assert!(community.channels().contains(&channel.id()));

        instance_b
            .delete_community_channel(community.id(), channel.id())
            .await?;
        let community = instance_a.get_community(community.id()).await?;
        assert!(!community.channels().contains(&channel.id()));
        Ok(())
    }

    #[async_test]
    async fn unauthorized_edit_community_name() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_name".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .edit_community_name(community.id(), "Community1")
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_name() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_name".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(community.id(), CommunityPermission::EditInfo, role.id())
            .await?;

        let new_name = "Community1";
        instance_b
            .edit_community_name(community.id(), new_name)
            .await?;
        let community = instance_a.get_community(community.id()).await?;
        assert_eq!(new_name, community.name());
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_description() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_description".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .edit_community_description(community.id(), Some("description".to_string()))
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_description() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_description".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(community.id(), CommunityPermission::EditInfo, role.id())
            .await?;

        let new_description = Some("description".to_string());
        instance_b
            .edit_community_description(community.id(), new_description.clone())
            .await?;
        let community = instance_a.get_community(community.id()).await?;

        assert_eq!(new_description.as_deref(), community.description());
        Ok(())
    }
    #[async_test]
    async fn edit_community_description_as_creator() -> anyhow::Result<()> {
        let context = Some("test::edit_community_description_as_creator".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();

        let community = instance_a.create_community("Community0").await?;
        let new_description = Some("description".to_string());
        instance_a
            .edit_community_description(community.id(), new_description.clone())
            .await?;

        let community = instance_a.get_community(community.id()).await?;
        assert_eq!(community.description(), new_description.as_deref());
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_role_name() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_role_name".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .edit_community_role_name(community.id(), role.id(), "new_name".to_string())
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_role_name() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_role_name".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(community.id(), CommunityPermission::ManageRoles, role.id())
            .await?;

        let new_name = "new_name".to_string();
        instance_b
            .edit_community_role_name(community.id(), role.id(), new_name.clone())
            .await?;
        let role = instance_a
            .get_community_role(community.id(), role.id())
            .await?;
        assert_eq!(new_name, role.name());
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_permissions() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_permissions".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .grant_community_permission(community.id(), CommunityPermission::EditInfo, role.id())
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_permissions() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_permissions".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManagePermissions,
                role.id(),
            )
            .await?;

        instance_b
            .grant_community_permission(community.id(), CommunityPermission::EditInfo, role.id())
            .await?;
        let community = instance_a.get_community(community.id()).await?;
        assert!(community
            .permissions()
            .get(&CommunityPermission::EditInfo)
            .unwrap()
            .contains(&role.id()));
        Ok(())
    }
    #[async_test]
    async fn unauthorized_remove_community_member() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_remove_community_member".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![
            account_opts.clone(),
            account_opts.clone(),
            account_opts,
        ])
        .await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();
        let (instance_c, did_c, _) = &mut accounts[2].clone();

        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_c.clone()), None)
            .await?;

        let mut stream_b = instance_b.get_community_stream(community.id()).await?;
        instance_c
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_b, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_c.clone()
            }
        );

        let result = instance_b
            .remove_community_member(community.id(), did_c.clone())
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_remove_community_member() -> anyhow::Result<()> {
        let context = Some("test::authorized_remove_community_member".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![
            account_opts.clone(),
            account_opts.clone(),
            account_opts,
        ])
        .await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();
        let (instance_c, did_c, _) = &mut accounts[2].clone();

        let community = instance_a.create_community("Community0").await?;
        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManageMembers,
                role.id(),
            )
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let mut stream_b = instance_b.get_community_stream(community.id()).await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        assert_eq!(
            next_event(&mut stream_b, Duration::from_secs(60)).await?,
            MessageEventKind::GrantedCommunityRole {
                community_id: community.id(),
                role_id: role.id(),
                user: did_b.clone()
            }
        );
        let invite = instance_a
            .create_community_invite(community.id(), Some(did_c.clone()), None)
            .await?;
        assert_eq!(
            next_event(&mut stream_b, Duration::from_secs(60)).await?,
            MessageEventKind::CreatedCommunityInvite {
                community_id: community.id(),
                invite: invite.clone(),
            }
        );

        instance_c
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_b, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_c.clone()
            }
        );

        instance_b
            .remove_community_member(community.id(), did_c.clone())
            .await?;
        let community = instance_a.get_community(community.id()).await?;

        assert!(!community.members().contains(&did_c.clone()));
        Ok(())
    }

    #[async_test]
    async fn unauthorized_edit_community_channel_name() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_channel_name".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .edit_community_channel_name(community.id(), channel.id(), "new_name")
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_name() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_channel_name".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManageChannels,
                role.id(),
            )
            .await?;

        let new_name = "new_name";
        instance_b
            .edit_community_channel_name(community.id(), channel.id(), new_name)
            .await?;
        let channel = instance_a
            .get_community_channel(community.id(), channel.id())
            .await?;

        assert_eq!(new_name, channel.name());
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_channel_description() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_channel_description".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .edit_community_channel_description(
                community.id(),
                channel.id(),
                Some("description".to_string()),
            )
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_description() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_channel_description".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManageChannels,
                role.id(),
            )
            .await?;

        let new_description = Some("description".to_string());
        instance_b
            .edit_community_channel_description(
                community.id(),
                channel.id(),
                new_description.clone(),
            )
            .await?;
        let channel = instance_a
            .get_community_channel(community.id(), channel.id())
            .await?;

        assert_eq!(new_description.as_deref(), channel.description());
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_channel_permissions() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_channel_permissions".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;
        instance_a
            .revoke_community_channel_permission_for_all(
                community.id(),
                channel.id(),
                CommunityChannelPermission::ViewChannel,
            )
            .await?;

        let _ = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let result = instance_b
            .grant_community_channel_permission_for_all(
                community.id(),
                channel.id(),
                CommunityChannelPermission::ViewChannel,
            )
            .await;
        assert_eq!(
            format!("{:?}", result),
            format!("{:?}", Err::<Community, Error>(Error::Unauthorized))
        );
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_permissions() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_channel_permissions".into());
        let account_opts = (None, None, context);
        let accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0].clone();
        let (instance_b, did_b, _) = &mut accounts[1].clone();

        let community = instance_a.create_community("Community0").await?;
        let channel = instance_a
            .create_community_channel(community.id(), "Channel0", CommunityChannelType::Standard)
            .await?;
        instance_a
            .revoke_community_channel_permission_for_all(
                community.id(),
                channel.id(),
                CommunityChannelPermission::ViewChannel,
            )
            .await?;

        let invite = instance_a
            .create_community_invite(community.id(), Some(did_b.clone()), None)
            .await?;
        let mut stream_a = instance_a.get_community_stream(community.id()).await?;
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        assert_eq!(
            next_event(&mut stream_a, Duration::from_secs(60)).await?,
            MessageEventKind::AcceptedCommunityInvite {
                community_id: community.id(),
                invite_id: invite.id(),
                user: did_b.clone()
            }
        );

        let role = instance_a
            .create_community_role(community.id(), "Role0")
            .await?;
        instance_a
            .grant_community_role(community.id(), role.id(), did_b.clone())
            .await?;
        instance_a
            .grant_community_permission(
                community.id(),
                CommunityPermission::ManagePermissions,
                role.id(),
            )
            .await?;

        instance_b
            .grant_community_channel_permission_for_all(
                community.id(),
                channel.id(),
                CommunityChannelPermission::ViewChannel,
            )
            .await?;

        let channel = instance_a
            .get_community_channel(community.id(), channel.id())
            .await?;
        assert!(channel
            .permissions()
            .get(&CommunityChannelPermission::ViewChannel)
            .is_none());
        Ok(())
    }

    // #[async_test]
    // async fn unauthorized_view_community_channel_messages() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn authorized_view_community_channel_messages() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn unauthorized_send_community_channel_message() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn authorized_send_community_channel_message() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn unauthorized_delete_community_channel_message() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
    // #[async_test]
    // async fn authorized_delete_community_channel_message() -> anyhow::Result<()> {
    //     assert!(false);
    //     Ok(())
    // }
}

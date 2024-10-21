mod common;
#[cfg(test)]
mod test {
    use chrono::{Duration, Utc};
    use warp::raygun::community::{CommunityPermission, RayGunCommunity};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;

    use warp::error::Error;

    use crate::common::create_accounts;

    #[async_test]
    async fn get_community_as_uninvited_user() -> anyhow::Result<()> {
        let context = Some("test::get_community_as_uninvited_user".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;

        let (instance_a, did_a, _) = &mut accounts[0];
        let communities = vec![
            instance_a.create_community("Community0").await?, //with no invite
            instance_a.create_community("Community1").await?, //with expired invite
            instance_a.create_community("Community2").await?, //with invite targeting a different user
        ];
        instance_a
            .create_community_invite(
                communities[1].id(),
                None,
                Some(Utc::now() - Duration::days(1)),
            )
            .await?;
        instance_a
            .create_community_invite(communities[2].id(), Some(did_a.clone()), None)
            .await?;

        let (instance_b, _, _) = &accounts[1];
        for (i, community) in communities.iter().enumerate() {
            match instance_b.get_community(community.id()).await {
                Err(e) => match e {
                    Error::Unauthorized => {}
                    _ => panic!("error should be Error::Unauthorized"),
                },
                Ok(_) => panic!("should be unauthorized to get communities[{}])", i),
            }
        }
        Ok(())
    }
    #[async_test]
    async fn get_community_as_invited_user() -> anyhow::Result<()> {
        let context = Some("test::get_community_as_invited_user".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;

        let (_, did_b, _) = &accounts[1];
        let did_b = did_b.clone();
        let (instance_a, _, _) = &mut accounts[0];
        let communities = vec![
            instance_a.create_community("Community0").await?, //with open invite
            instance_a.create_community("Community1").await?, //with specifically targeted invite
        ];
        instance_a
            .create_community_invite(communities[0].id(), None, None)
            .await?;
        instance_a
            .create_community_invite(
                communities[1].id(),
                Some(did_b),
                Some(Utc::now() + Duration::days(1)),
            )
            .await?;

        let (instance_b, _, _) = &accounts[1];
        for (i, community) in communities.iter().enumerate() {
            let result = instance_b.get_community(community.id()).await;
            assert!(
                result.is_ok(),
                "should successfully get communities[{}])",
                i
            );
        }
        Ok(())
    }
    #[async_test]
    async fn get_community_as_member() -> anyhow::Result<()> {
        let context = Some("test::get_community_as_member".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;

        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), None, None)
            .await?;

        let (instance_b, _, _) = &mut accounts[1];
        instance_b
            .accept_community_invite(community.id(), invite.id())
            .await?;
        instance_b.get_community(community.id()).await?;
        Ok(())
    }

    #[async_test]
    async fn delete_community_as_creator() -> anyhow::Result<()> {
        let context = Some("test::delete_community_as_creator".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts]).await?;

        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        instance_a.delete_community(community.id()).await?;
        Ok(())
    }
    #[async_test]
    async fn delete_community_as_non_creator() -> anyhow::Result<()> {
        let context = Some("test::delete_community_as_non_creator".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;

        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;

        let (instance_b, _, _) = &mut accounts[1];
        match instance_b.delete_community(community.id()).await {
            Err(e) => match e {
                Error::Unauthorized => {}
                _ => panic!("error should be Error::Unauthorized"),
            },
            Ok(_) => panic!("should be unauthorized to delete community"),
        }
        Ok(())
    }

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
    async fn get_community_invite() -> anyhow::Result<()> {
        let context = Some("test::get_community_invite".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;

        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a
            .create_community_invite(community.id(), None, None)
            .await?;

        let (instance_b, _, _) = &accounts[1];
        instance_b
            .get_community_invite(community.id(), invite.id())
            .await?;
        Ok(())
    }
    #[async_test]
    async fn unauthorized_create_community_invite() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_create_community_invite".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;

        let (instance_b, _, _) = &mut accounts[1];
        match instance_b
            .create_community_invite(community.id(), None, None)
            .await
        {
            Err(e) => match e {
                Error::Unauthorized => {}
                _ => panic!("error should be Error::Unauthorized"),
            },
            Ok(_) => panic!("should be unauthorized to delete community"),
        }
        Ok(())
    }
    #[async_test]
    async fn authorized_create_community_invite() -> anyhow::Result<()> {
        let context = Some("test::authorized_create_community_invite".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts.clone(), account_opts]).await?;
        
        let (_, did_b, _) = &accounts[1];
        let did_b = did_b.clone();
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        let role = instance_a.create_community_role(community.id(), "Role0").await?;
        instance_a.grant_community_role(community.id(), role.id(), did_b).await?;
        instance_a.grant_community_permission(community.id(), CommunityPermission::ManageInvites, role.id()).await?;

        let (instance_b, _, _) = &mut accounts[1];
        let invite = instance_b
            .create_community_invite(community.id(), None, None)
            .await?;

        let (instance_c, _, _) = &mut accounts[2];
        instance_c.accept_community_invite(community.id(), invite.id()).await?;
        Ok(())
    }
    #[async_test]
    async fn unauthorized_delete_community_invite() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_delete_community_invite".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        let invite = instance_a.create_community_invite(community.id(), None, None).await?;
        
        let (instance_b, _, _) = &mut accounts[0];
        match instance_b.delete_community_invite(community.id(), invite.id()).await {
            Err(e) => match e {
                Error::Unauthorized => {}
                _ => panic!("error should be Error::Unauthorized"),
            },
            Ok(_) => panic!("should be unauthorized to delete community"),
        }
        Ok(())
    }
    #[async_test]
    async fn authorized_delete_community_invite() -> anyhow::Result<()> {
        let context = Some("test::authorized_delete_community_invite".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        
        let (_, did_b, _) = &accounts[1];
        let did_b = did_b.clone();
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        let role = instance_a.create_community_role(community.id(), "Role0").await?;
        instance_a.grant_community_role(community.id(), role.id(), did_b).await?;
        instance_a.grant_community_permission(community.id(), CommunityPermission::ManageInvites, role.id()).await?;
        let invite = instance_a.create_community_invite(community.id(), None, None).await?;
        
        let (instance_b, _, _) = &mut accounts[1];
        instance_b.delete_community_invite(community.id(), invite.id()).await?;
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_invite() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_invite".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_invite() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_invite".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn accept_valid_community_invitation() -> anyhow::Result<()> {
        let context = Some("test::accept_valid_community_invitation".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn try_accept_invalid_community_invitation() -> anyhow::Result<()> {
        let context = Some("test::try_accept_invalid_community_invitation".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn try_accept_expired_community_invitation() -> anyhow::Result<()> {
        let context = Some("test::try_accept_expired_community_invitation".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }

    #[async_test]
    async fn unauthorized_get_community_channel() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_get_community_channel".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_get_community_channel() -> anyhow::Result<()> {
        let context = Some("test::authorized_get_community_channel".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_create_community_channel() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_create_community_channel".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_create_community_channel() -> anyhow::Result<()> {
        let context = Some("test::authorized_create_community_channel".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_delete_community_channel() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_delete_community_channel".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_delete_community_channel() -> anyhow::Result<()> {
        let context = Some("test::authorized_delete_community_channel".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }

    #[async_test]
    async fn unauthorized_edit_community_name() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_name".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_name() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_name".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_description() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_description".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_description() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_description".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_roles() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_roles".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_roles() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_roles".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_permissions() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_permissions".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_permissions() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_permissions".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_remove_community_member() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_remove_community_member".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_remove_community_member() -> anyhow::Result<()> {
        let context = Some("test::authorized_remove_community_member".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }

    #[async_test]
    async fn unauthorized_edit_community_channel_name() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_channel_name".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_name() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_channel_name".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_channel_description() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_channel_description".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_description() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_channel_description".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_channel_permissions() -> anyhow::Result<()> {
        let context = Some("test::unauthorized_edit_community_channel_permissions".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_permissions() -> anyhow::Result<()> {
        let context = Some("test::authorized_edit_community_channel_permissions".into());
        let account_opts = (None, None, context);
        let mut accounts = create_accounts(vec![account_opts.clone(), account_opts]).await?;
        let (instance_a, _, _) = &mut accounts[0];
        let community = instance_a.create_community("Community0").await?;
        assert!(false);
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

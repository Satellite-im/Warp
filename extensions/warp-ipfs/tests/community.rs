mod common;
#[cfg(test)]
mod test {
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
    async fn get_community_as_uninvited_user() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    async fn get_community_as_invited_user() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    async fn get_community_as_member() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }

    #[async_test]
    async fn delete_community_as_creator() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn delete_community_as_non_creator() -> anyhow::Result<()> {
        assert!(false);
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
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_create_community_invite() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_create_community_invite() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_delete_community_invite() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_delete_community_invite() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_invite() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_invite() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn accept_valid_community_invitation() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn try_accept_invalid_community_invitation() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn try_accept_expired_community_invitation() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }

    #[async_test]
    async fn unauthorized_get_community_channel() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_get_community_channel() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_create_community_channel() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_create_community_channel() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_delete_community_channel() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_delete_community_channel() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }

    #[async_test]
    async fn unauthorized_edit_community_name() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_name() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_description() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_description() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_roles() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_roles() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_permissions() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_permissions() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_remove_community_member() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_remove_community_member() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }

    #[async_test]
    async fn unauthorized_edit_community_channel_name() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_name() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_channel_description() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_description() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn unauthorized_edit_community_channel_permissions() -> anyhow::Result<()> {
        assert!(false);
        Ok(())
    }
    #[async_test]
    async fn authorized_edit_community_channel_permissions() -> anyhow::Result<()> {
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

mod common;

#[cfg(test)]
mod test {

    use futures::{stream, StreamExt, TryStreamExt};

    use crate::common::{create_account, PROFILE_IMAGE};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as async_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg(not(target_arch = "wasm32"))]
    use tokio::test as async_test;
    use warp::constellation::Constellation;

    #[async_test]
    async fn create_directory() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.create_directory("data", false).await?;

        assert!(root_directory.has_item("data"));
        Ok(())
    }

    #[async_test]
    async fn upload_file() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));
        Ok(())
    }

    #[async_test]
    async fn upload_file_stream() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        let stream = stream::iter(vec![Ok(PROFILE_IMAGE.into())]).boxed();
        let mut status = fs.put_stream("image.png", None, stream).await?;
        while let Some(progress) = status.next().await {
            match progress {
                warp::constellation::Progression::ProgressComplete { name, total } => {
                    assert_eq!(name, "image.png");
                    assert_eq!(total, Some(PROFILE_IMAGE.len()))
                }
                warp::constellation::Progression::ProgressFailed { .. } => {
                    unreachable!("should not fail")
                }
                _ => {}
            }
        }
        assert!(root_directory.has_item("image.png"));
        let item = root_directory.get_item("image.png")?;
        assert!(!item.thumbnail().is_empty());
        Ok(())
    }

    #[async_test]
    async fn get_file() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));

        let data = fs.get_buffer("image.png").await?;

        assert_eq!(data, PROFILE_IMAGE);
        Ok(())
    }

    #[async_test]
    async fn get_file_stream() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));

        let stream = fs.get_stream("image.png").await?;
        let data = stream.try_collect::<Vec<_>>().await?.concat();
        assert_eq!(data, PROFILE_IMAGE);
        Ok(())
    }

    #[async_test]
    async fn upload_file_to_directory() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.create_directory("images", false).await?;
        fs.put_buffer("/images/image.png", PROFILE_IMAGE).await?;
        let item = root_directory.get_item_by_path("/images/image.png")?;
        // Note: Checking the item name would be sure that the file was actually uploaded to the directory
        //       and not just be named after the directory
        assert_eq!(item.name(), "image.png");
        assert!(item.is_file());
        Ok(())
    }

    #[async_test]
    async fn rename_file_via_constellation() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));

        fs.rename("image.png", "icon.png").await?;

        assert!(root_directory.has_item("icon.png"));
        Ok(())
    }

    #[async_test]
    async fn rename_directory_via_constellation() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.create_directory("data", false).await?;

        assert!(root_directory.has_item("data"));

        fs.rename("data", "mystorage").await?;

        assert!(root_directory.has_item("mystorage"));
        Ok(())
    }

    #[async_test]
    async fn rename_file_in_directory() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.create_directory("/my/storage", true).await?;

        fs.put_buffer("/my/storage/image.png", PROFILE_IMAGE)
            .await?;

        fs.rename("/my/storage/image.png", "item.png").await?;

        assert!(root_directory
            .get_item_by_path("/my/storage/item.png")
            .is_ok());
        Ok(())
    }

    #[async_test]
    async fn remove_file() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));

        fs.remove("image.png", false).await?;

        assert!(!root_directory.has_item("image.png"));
        Ok(())
    }

    #[async_test]
    async fn check_thumbnail_of_file() -> anyhow::Result<()> {
        let (mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        let data: &[u8] = b"hello, world!";
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;
        fs.put_buffer("data.txt", data).await?;

        assert!(root_directory.has_item("image.png"));
        assert!(root_directory.has_item("data.txt"));
        //because this is an image, we should check to see if a thumbnail was produced
        let item = root_directory.get_item("image.png")?;
        assert!(!item.thumbnail().is_empty());
        let item = root_directory.get_item("data.txt")?;
        assert!(item.thumbnail().is_empty());
        Ok(())
    }
}

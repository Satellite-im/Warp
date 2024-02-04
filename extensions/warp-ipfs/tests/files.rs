mod common;

#[cfg(test)]
mod test {

    use futures::{stream, StreamExt, TryStreamExt};

    use crate::common::{create_account, PROFILE_IMAGE};

    #[tokio::test]
    async fn create_directory() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.create_directory("data", false).await?;

        assert!(root_directory.has_item("data"));
        Ok(())
    }

    #[tokio::test]
    async fn upload_file() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));
        Ok(())
    }

    #[tokio::test]
    async fn upload_file_stream() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        let stream = stream::once(async { PROFILE_IMAGE.to_vec() }).boxed();
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
        Ok(())
    }

    #[tokio::test]
    async fn get_file() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));

        let data = fs.get_buffer("image.png").await?;

        assert_eq!(data, PROFILE_IMAGE);
        Ok(())
    }

    #[tokio::test]
    async fn get_file_stream() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));

        let stream = fs.get_stream("image.png").await?;
        let data = stream.try_collect::<Vec<_>>().await?.concat();
        assert_eq!(data, PROFILE_IMAGE);
        Ok(())
    }

    #[tokio::test]
    async fn upload_file_to_directory() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
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

    #[tokio::test]
    async fn rename_file_via_constellation() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));

        fs.rename("image.png", "icon.png").await?;

        assert!(root_directory.has_item("icon.png"));
        Ok(())
    }

    #[tokio::test]
    async fn rename_directory_via_constellation() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.create_directory("data", false).await?;

        assert!(root_directory.has_item("data"));

        fs.rename("data", "mystorage").await?;

        assert!(root_directory.has_item("mystorage"));
        Ok(())
    }

    #[tokio::test]
    async fn rename_file_in_directory() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
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

    #[tokio::test]
    async fn remove_file() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;

        assert!(root_directory.has_item("image.png"));

        fs.remove("image.png", false).await?;

        assert!(!root_directory.has_item("image.png"));
        Ok(())
    }

    #[tokio::test]
    async fn check_thumbnail_of_file() -> anyhow::Result<()> {
        let (_, mut fs, _, _) = create_account(None, None, None).await?;
        let root_directory = fs.root_directory();
        fs.put_buffer("image.png", PROFILE_IMAGE).await?;
        fs.put_buffer("data.txt", &b"hello, world!"[..]).await?;

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

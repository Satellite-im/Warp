use futures::StreamExt;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::io::{ReaderStream, StreamReader};
use warp_ipfs::WarpIpfsBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (mut account, _, mut filesystem) = WarpIpfsBuilder::default().finalize().await;

    account
        .tesseract()
        .unlock(b"this is my totally secured password that should nnever be embedded in code")?;

    account.create_identity(None, None).await?;

    let fs = tokio::fs::File::open("/Users/dariusclark/Downloads/2024-07-21-114536.png").await?;

    let size = fs.metadata().await?.len();

    let st = ReaderStream::new(fs)
        .map(|result| result.map(|b| b.to_vec()))
        .boxed();

    let mut st = filesystem
        .put_stream("2024-07-21-114536.png", Some(size as _), st)
        .await?;

    while let Some(_e) = st.next().await {
        println!("{_e:?}")
    }

    let item = filesystem
        .root_directory()
        .get_item("2024-07-21-114536.png")?;

    println!("readme.txt: {item:?}");

    let data = item.thumbnail();

    tokio::fs::write("thumbnail.png", data).await?;

    Ok(())
}

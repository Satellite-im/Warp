// use clap::Parser;
// use futures::StreamExt;
// use std::path::{Path, PathBuf};
// use tracing_subscriber::EnvFilter;
// use warp::constellation::Constellation;
// use warp::crypto::zeroize::Zeroizing;
// use warp::crypto::DID;
// use warp::multipass::MultiPass;
// use warp::pocket_dimension::PocketDimension;
// use warp::raygun::RayGun;
// use warp::sync::{Arc, RwLock};
// use warp::tesseract::Tesseract;
// use warp_fs_ipfs::config::FsIpfsConfig;
// use warp_fs_ipfs::{IpfsFileSystem, Persistent as FsPersistent, Temporary as FsTemporary};
// use warp_mp_ipfs::{ipfs_identity_persistent, ipfs_identity_temporary};
// use warp_pd_flatfile::FlatfileStorage;
// use warp_pd_stretto::StrettoClient;
// use warp_rg_ipfs::config::RgIpfsConfig;
// use warp_rg_ipfs::IpfsMessaging;
// use warp_rg_ipfs::Persistent;
// use warp_rg_ipfs::Temporary;

// #[derive(Debug, Parser)]
// #[clap(name = "")]
// struct Opt {
//     #[clap(long)]
//     path: Option<PathBuf>,
//     #[clap(long)]
//     with_key: bool,
//     #[clap(long)]
//     experimental_node: bool,
//     #[clap(long)]
//     stdout_log: bool,
// }

// fn cache_setup(root: Option<PathBuf>) -> anyhow::Result<Arc<RwLock<Box<dyn PocketDimension>>>> {
//     if let Some(root) = root {
//         let storage = FlatfileStorage::new_with_index_file(root, PathBuf::from("cache-index"))?;
//         return Ok(Arc::new(RwLock::new(Box::new(storage))));
//     }
//     let storage = StrettoClient::new()?;
//     Ok(Arc::new(RwLock::new(Box::new(storage))))
// }

// async fn create_account<P: AsRef<Path>>(
//     path: Option<P>,
//     cache: Arc<RwLock<Box<dyn PocketDimension>>>,
//     passphrase: Zeroizing<String>,
//     experimental: bool,
// ) -> anyhow::Result<Box<dyn MultiPass>> {
//     let tesseract = match path.as_ref() {
//         Some(path) => {
//             let path = path.as_ref();
//             let tesseract = Tesseract::from_file(path.join("tesseract_store")).unwrap_or_default();
//             tesseract.set_file(path.join("tesseract_store"));
//             tesseract.set_autosave();
//             tesseract
//         }
//         None => Tesseract::default(),
//     };

//     tesseract.unlock(passphrase.as_bytes())?;

//     let config = match path.as_ref() {
//         Some(path) => warp_mp_ipfs::config::MpIpfsConfig::production(path, experimental),
//         None => warp_mp_ipfs::config::MpIpfsConfig::testing(experimental),
//     };

//     let mut account: Box<dyn MultiPass> = match path.is_some() {
//         true => Box::new(ipfs_identity_persistent(config, tesseract, Some(cache)).await?),
//         false => Box::new(ipfs_identity_temporary(Some(config), tesseract, Some(cache)).await?),
//     };

//     if account.get_own_identity().await.is_err() {
//         account.create_identity(None, None).await?;
//     }
//     Ok(account)
// }

// async fn create_fs<P: AsRef<Path>>(
//     account: Box<dyn MultiPass>,
//     path: Option<P>,
// ) -> anyhow::Result<Box<dyn Constellation>> {
//     let config = match path.as_ref() {
//         Some(path) => FsIpfsConfig::production(path),
//         None => FsIpfsConfig::testing(),
//     };

//     let filesystem: Box<dyn Constellation> = match path.is_some() {
//         true => Box::new(IpfsFileSystem::<FsPersistent>::new(account, Some(config)).await?),
//         false => Box::new(IpfsFileSystem::<FsTemporary>::new(account, Some(config)).await?),
//     };

//     Ok(filesystem)
// }

// #[allow(dead_code)]
// async fn create_rg(
//     path: Option<PathBuf>,
//     account: Box<dyn MultiPass>,
//     filesystem: Option<Box<dyn Constellation>>,
//     cache: Arc<RwLock<Box<dyn PocketDimension>>>,
// ) -> anyhow::Result<Box<dyn RayGun>> {
//     let chat = match path.as_ref() {
//         Some(path) => {
//             let config = RgIpfsConfig::production(path);
//             Box::new(
//                 IpfsMessaging::<Persistent>::new(Some(config), account, filesystem, Some(cache))
//                     .await?,
//             ) as Box<dyn RayGun>
//         }
//         None => {
//             Box::new(IpfsMessaging::<Temporary>::new(None, account, filesystem, Some(cache)).await?)
//                 as Box<dyn RayGun>
//         }
//     };

//     Ok(chat)
// }

// #[allow(clippy::clone_on_copy)]
// #[allow(clippy::await_holding_lock)]
// #[tokio::main]
// async fn main() -> anyhow::Result<()> {
//     let opt = Opt::parse();
//     if fdlimit::raise_fd_limit().is_none() {}

//     if !opt.stdout_log {
//         let file_appender = tracing_appender::rolling::hourly(
//             opt.path.clone().unwrap_or_else(|| PathBuf::from("./")),
//             "warp_rg_ipfs_messenger.log",
//         );
//         let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

//         tracing_subscriber::fmt()
//             .with_writer(non_blocking)
//             .with_env_filter(EnvFilter::from_default_env())
//             .init();
//     } else {
//         tracing_subscriber::fmt()
//             .with_env_filter(EnvFilter::from_default_env())
//             .init();
//     }

//     let cache = cache_setup(opt.path.as_ref().map(|p| p.join("cache")))?;
//     let password = if opt.with_key {
//         rpassword::prompt_password("Enter A Password: ")?
//     } else {
//         "embedded pass".into()
//     };

//     println!("Creating or obtaining account...");
//     let new_account = create_account(
//         opt.path.clone(),
//         cache.clone(),
//         Zeroizing::new(password),
//         opt.experimental_node,
//     )
//     .await?;

//     println!("Initializing Constellation");
//     let fs = create_fs(new_account.clone(), opt.path.clone()).await?;

//     println!("Initializing RayGun");
//     let mut _chat = create_rg(
//         opt.path.clone(),
//         new_account.clone(),
//         Some(fs.clone()),
//         cache,
//     )
//     .await?;

//     let did = DID::default();

//     let mut sub = _chat.subscribe().await?;

//     tokio::spawn(async move {
//         while let Some(e) = sub.next().await {
//             println!("{:?}", e);
//         }
//     });

//     let conversation = _chat.create_conversation(&did).await?;

//     loop {
//         tokio::time::sleep(std::time::Duration::from_millis(50)).await
//     }

//     // let mut count = 0;
//     // loop {
//     //     chat.send(conversation.id(), None, vec!["a".repeat(2048)]).await?;
//     //     count += 1;
//     //     println!("Count {count}");
//     //     tokio::time::sleep(std::time::Duration::from_millis(50)).await;
//     // }
//     Ok(())
// }

fn main() {}

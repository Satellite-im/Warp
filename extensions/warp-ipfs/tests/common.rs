use futures::{stream, StreamExt};
use rust_ipfs::{Ipfs, Multiaddr, PeerId};
use warp::{
    crypto::DID,
    multipass::{identity::Identity, MultiPass},
    raygun::RayGun,
    tesseract::Tesseract,
};
use warp_ipfs::{
    config::{Bootstrap, Discovery},
    WarpIpfsBuilder,
};

pub async fn node_info(nodes: Vec<Ipfs>) -> Vec<(Ipfs, PeerId, Vec<Multiaddr>)> {
    stream::iter(nodes)
        .filter_map(|node| async move {
            let (peer_id, addrs) = node
                .identity(None)
                .await
                .map(|peer| (peer.peer_id, peer.listen_addrs))
                .expect("Expect own identity");

            Some((node, peer_id, addrs))
        })
        .collect::<Vec<_>>()
        .await
}

pub async fn mesh_connect(nodes: Vec<Ipfs>) -> anyhow::Result<()> {
    let nodes = node_info(nodes).await;
    let count = nodes.len();

    for i in 0..count {
        for (j, (_, _, addrs)) in nodes.iter().enumerate() {
            if i != j {
                for addr in addrs {
                    if let Err(_e) = nodes[i].0.connect(addr.clone()).await {}
                }
            }
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub async fn create_account(
    username: Option<&str>,
    passphrase: Option<&str>,
    context: Option<String>,
) -> anyhow::Result<(Box<dyn MultiPass>, DID, Identity)> {
    let tesseract = Tesseract::default();
    tesseract.unlock(b"internal pass").unwrap();
    let mut config = warp_ipfs::config::Config::development();
    config.listen_on = vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()];
    config.store_setting.discovery = Discovery::Provider(context);
    config.store_setting.share_platform = true;
    config.ipfs_setting.relay_client.relay_address = vec![];
    config.ipfs_setting.bootstrap = false;
    config.bootstrap = Bootstrap::None;

    let (mut account, _, _) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .set_config(config)
        .finalize()
        .await?;
    let profile = account.create_identity(username, passphrase).await?;
    let identity = profile.identity().clone();

    Ok((account, identity.did_key(), identity))
}

#[allow(dead_code)]
pub async fn create_accounts(
    infos: Vec<(Option<&str>, Option<&str>, Option<String>)>,
) -> anyhow::Result<Vec<(Box<dyn MultiPass>, DID, Identity)>> {
    let mut accounts = vec![];
    let mut nodes = vec![];
    for (username, passphrase, context) in infos {
        let account = create_account(username, passphrase, context).await?;
        let ipfs = account
            .0
            .handle()
            .expect("Handle accessible")
            .downcast_ref::<Ipfs>()
            .cloned()
            .unwrap();
        nodes.push(ipfs);
        accounts.push(account);
    }

    mesh_connect(nodes).await?;

    Ok(accounts)
}

pub async fn create_account_and_chat(
    username: Option<&str>,
    passphrase: Option<&str>,
    context: Option<String>,
) -> anyhow::Result<(Box<dyn MultiPass>, Box<dyn RayGun>, DID, Identity)> {
    let tesseract = Tesseract::default();
    tesseract.unlock(b"internal pass").unwrap();
    let mut config = warp_ipfs::config::Config::development();
    config.listen_on = vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()];
    config.store_setting.discovery = Discovery::Provider(context);
    config.store_setting.share_platform = true;
    config.ipfs_setting.relay_client.relay_address = vec![];
    config.ipfs_setting.bootstrap = false;
    config.bootstrap = Bootstrap::None;

    let (mut account, raygun, _) = WarpIpfsBuilder::default()
        .set_tesseract(tesseract)
        .set_config(config)
        .finalize()
        .await?;

    let profile = account.create_identity(username, passphrase).await?;
    let identity = profile.identity().clone();

    Ok((account, raygun, identity.did_key(), identity))
}

pub async fn create_accounts_and_chat(
    infos: Vec<(Option<&str>, Option<&str>, Option<String>)>,
) -> anyhow::Result<Vec<(Box<dyn MultiPass>, Box<dyn RayGun>, DID, Identity)>> {
    let mut accounts = vec![];
    let mut nodes = vec![];
    for (username, passphrase, context) in infos {
        let account = create_account_and_chat(username, passphrase, context).await?;
        let ipfs = account
            .0
            .handle()
            .expect("Handle accessible")
            .downcast_ref::<Ipfs>()
            .cloned()
            .unwrap();
        nodes.push(ipfs);
        accounts.push(account);
    }

    mesh_connect(nodes).await?;

    Ok(accounts)
}

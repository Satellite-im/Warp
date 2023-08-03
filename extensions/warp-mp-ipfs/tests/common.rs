use futures::{stream, StreamExt};
use rust_ipfs::{Ipfs, Multiaddr, PeerId};
use warp::{
    crypto::DID,
    multipass::{identity::Identity, MultiPass},
    tesseract::Tesseract,
};
use warp_mp_ipfs::{
    config::{Bootstrap, Discovery},
    IpfsIdentity,
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

pub async fn create_account(
    username: Option<&str>,
    passphrase: Option<&str>,
    context: Option<String>,
) -> anyhow::Result<(Box<dyn MultiPass>, DID, Identity)> {
    let tesseract = Tesseract::default();
    tesseract.unlock(b"internal pass").unwrap();
    let mut config = warp_mp_ipfs::config::MpIpfsConfig::development();
    config.listen_on = vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()];
    config.store_setting.discovery = Discovery::Provider(context);
    config.store_setting.share_platform = true;
    config.ipfs_setting.relay_client.relay_address = vec![];
    config.ipfs_setting.bootstrap = false;
    config.bootstrap = Bootstrap::None;

    let mut account = IpfsIdentity::new(config, tesseract).await?;
    let did = account.create_identity(username, passphrase).await?;
    let identity = account.get_own_identity().await?;
    Ok((Box::new(account), did, identity))
}

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

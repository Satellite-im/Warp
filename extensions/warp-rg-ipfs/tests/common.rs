use futures::{stream, StreamExt};
use rust_ipfs::{Ipfs, Multiaddr, PeerId};
use warp::{
    crypto::DID,
    multipass::{identity::Identity, MultiPass},
    raygun::RayGun,
    tesseract::Tesseract,
};
use warp_mp_ipfs::{
    config::{Bootstrap, Discovery},
    IpfsIdentity,
};
use warp_rg_ipfs::IpfsMessaging;

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
        for (j, (_, peer_id, addrs)) in nodes.iter().enumerate() {
            if i != j {
                for addr in addrs {
                    if let Err(_e) = nodes[i].0.add_peer(*peer_id, addr.clone()).await {}
                    if let Err(_e) = nodes[i].0.connect(*peer_id).await {}
                }
            }
        }
    }

    Ok(())
}

pub async fn create_account_and_chat(
    username: Option<&str>,
    passphrase: Option<&str>,
    context: Option<String>,
) -> anyhow::Result<(Box<dyn MultiPass>, Box<dyn RayGun>, DID, Identity)> {
    let tesseract = Tesseract::default();
    tesseract.unlock(b"internal pass").unwrap();
    let mut config = warp_mp_ipfs::config::MpIpfsConfig::development();
    config.listen_on = vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()];
    config.store_setting.discovery = Discovery::Provider(context);
    config.store_setting.share_platform = true;
    config.ipfs_setting.relay_client.relay_address = vec![];
    config.ipfs_setting.bootstrap = false;
    config.bootstrap = Bootstrap::None;

    let mut account =
        Box::new(IpfsIdentity::new(config, tesseract).await?) as Box<dyn MultiPass>;
    let did = account.create_identity(username, passphrase).await?;
    let identity = account.get_own_identity().await?;
    let raygun = Box::new(IpfsMessaging::new(None, account.clone(), None).await?) as Box<_>;
    Ok((account, raygun, did, identity))
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

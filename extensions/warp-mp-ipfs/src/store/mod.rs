pub mod friends;
pub mod identity;

pub const IDENTITY_BROADCAST: &'static str = "identity/broadcast";
pub const FRIENDS_BROADCAST: &'static str = "friends/broadcast";


// This provides a future that stores the topic as a dag in a "gossipsub:<topic>" format and provide the cid over DHT and also get providers of the same cid
// who are providing and connect to them.
// TODO: Investigate the delay in providing the CID 
pub async fn topic_discovery<S: AsRef<str>>(ipfs: ipfs::Ipfs<ipfs::Types>, topic: S) -> anyhow::Result<impl futures::Future<Output = ()>> {
    let topic = topic.as_ref();
    let cid = ipfs.put_dag(libipld::ipld!(format!("gossipsub:{}", topic))).await?;
    ipfs.provide(cid).await?;

    Ok(async move {
        loop {
            if let Ok(list) = ipfs.get_providers(cid).await {
                for peer in list {
                    // Check to see if we are already connected to the peer
                    if let Ok(connections) = ipfs.peers().await {
                        if connections.iter().filter(|connection| connection.addr.peer_id == peer).count() >= 1 {
                            continue
                        }
                    }

                    // Get address(es) of peer and connect to them
                    // TODO: Maybe use a relay or give an option when connecting?
                    if let Ok(addrs) = ipfs.find_peer(peer).await {
                        for addr in addrs {
                            let addr = addr.with(ipfs::Protocol::P2p(peer.into()));
                            if let Ok(addr) = addr.try_into() {
                                if let Err(_e) = ipfs.connect(addr).await {
                                    //TODO: Log
                                    continue
                                }
                            }
                        }
                    }
                }
            }
        }
    })
} 
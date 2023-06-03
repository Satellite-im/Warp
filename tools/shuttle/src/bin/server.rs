use rust_ipfs::{
    p2p::{
        IdentifyConfiguration, PeerInfo, PubsubConfig, TransportConfig, UpdateMode, UpgradeVersion,
    },
    UninitializedIpfs,
};
use shuttle::{server::ShuttleServer, store::Store};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ipfs = UninitializedIpfs::default()
        .enable_mdns()
        .fd_limit(rust_ipfs::FDLimit::Max)
        .enable_upnp()
        .enable_relay(true)
        .enable_relay_server(None)
        .disable_delay()
        .set_pubsub_configuration(PubsubConfig {
            floodsub_compat: true,
            max_transmit_size: 8 * 1024 * 1024,
            ..Default::default()
        })
        .set_transport_configuration(TransportConfig {
            version: Some(UpgradeVersion::Lazy),
            yamux_update_mode: UpdateMode::Read,
            yamux_max_buffer_size: 4 * 1024 * 1024,
            yamux_receive_window_size: 256 * 1024,
            ..Default::default()
        })
        .set_identify_configuration(IdentifyConfiguration {
            agent_version: "/p2p/shuttle/0.1".into(),
            ..Default::default()
        })
        .start()
        .await?;
    
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let PeerInfo {
        peer_id,
        listen_addrs: addresses,
        ..
    } = ipfs.identity(None).await?;

    println!("PeerID: {}", peer_id);

    for address in addresses {
        println!("Listening on: {address}");
    }

    let _service = ShuttleServer::new(ipfs.clone(), MemoryStore::default());

    tokio::sync::Notify::new().notified().await;
    Ok(())
}

use std::{
    collections::{hash_map::Entry, HashMap},
};

#[derive(Default)]
pub struct MemoryStore {
    inner: HashMap<Vec<u8>, Vec<Vec<u8>>>,
}

#[async_trait::async_trait]
impl Store for MemoryStore {
    async fn insert(&mut self, key: &[u8], value: &[u8]) -> Result<(), anyhow::Error> {
        match self.inner.entry(key.into()) {
            Entry::Vacant(e) => {
                e.insert(vec![value.into()]);
            }
            Entry::Occupied(mut e) => {
                let entry = e.get_mut();
                if !entry.contains(&value.into()) {
                    entry.push(value.into());
                }
            }
        }

        Ok(())
    }

    async fn find(&self, key: &[u8]) -> Result<Vec<Vec<u8>>, anyhow::Error> {
        self.inner
            .get(key)
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.clone())
                    .collect::<Vec<_>>()
            })
            .ok_or(anyhow::anyhow!("No entry found"))
    }

    async fn replace(&mut self, _: &[u8], _: &[u8]) -> Result<(), anyhow::Error> {
        anyhow::bail!("Unimplemented")
    }

    async fn remove(&mut self, key: &[u8]) -> Result<(), anyhow::Error> {
        self.inner.remove(key);
        Ok(())
    }
}

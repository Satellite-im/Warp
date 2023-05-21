use rust_ipfs::{
    p2p::{IdentifyConfiguration, PubsubConfig, TransportConfig, UpdateMode, UpgradeVersion},
    UninitializedIpfs,
};
// use shuttle::server::ShuttleServer;

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

    // let memory_store = MemoryStore::default();
    // let _service = ShuttleServer::new(ipfs.clone(), memory_store);

    tokio::sync::Notify::new().notified().await;
    Ok(())
}

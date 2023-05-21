use rust_ipfs::{
    p2p::{PubsubConfig, TransportConfig, UpdateMode, UpgradeVersion},
    UninitializedIpfs,
};
use shuttle::client::ShuttleClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ipfs = UninitializedIpfs::default()
        .enable_mdns()
        .fd_limit(rust_ipfs::FDLimit::Max)
        .enable_upnp()
        .enable_relay(true)
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
        .start()
        .await?;

    let _client = ShuttleClient::new(ipfs);

    //TODO:
    Ok(())
}

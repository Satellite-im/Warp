use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Dcutr {
    pub enable: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Mdns {
    pub enable: bool,
    pub enable_ipv6: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Autonat {
    pub enable: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RelayClient {
    pub enable: bool,
    pub relay_address: Option<Multiaddr>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RelayServer {
    pub enable: bool,
    pub relay_address: Option<Multiaddr>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct BehaviourConfig {
    pub mdns: Mdns,
    pub autonat: Autonat,
    pub relay_client: RelayClient,
    pub relay_server: RelayServer,
    pub dcutr: Dcutr,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub bootstrap: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
    pub behaviour: BehaviourConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bootstrap: vec![
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
            ]
            .iter()
            .filter_map(|s| Multiaddr::from_str(s).ok())
            .collect::<Vec<_>>(),

            listen_on: vec!["/ip4/0.0.0.0/tcp/0".parse().unwrap()],
            behaviour: BehaviourConfig {
                mdns: Mdns {
                    enable: true,
                    enable_ipv6: true,
                },
                autonat: Autonat { enable: true },
                relay_client: RelayClient {
                    enable: false,
                    relay_address: None,
                },
                relay_server: RelayServer {
                    enable: false,
                    relay_address: None,
                },
                dcutr: Dcutr { enable: true },
            },
        }
    }
}

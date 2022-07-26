use ipfs::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Dcutr {
    pub enable: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Mdns {
    pub enable: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Autonat {
    pub enable: bool,
    pub servers: Vec<Multiaddr>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RelayClient {
    pub enable: bool,
    pub relay_address: Option<Multiaddr>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RelayServer {
    pub enable: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Rendezvous {
    pub enable: bool,
    pub address: Multiaddr,
}

impl Default for Rendezvous {
    fn default() -> Self {
        Self {
            enable: false,
            address: Multiaddr::empty(),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct IpfsSetting {
    pub temporary: bool,
    pub path: Option<PathBuf>,
    pub bootstrap: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
    pub mdns: Mdns,
    pub autonat: Autonat,
    pub relay_client: RelayClient,
    pub relay_server: RelayServer,
    pub dcutr: Dcutr,
    pub rendezvous: Rendezvous,
    pub store_setting: StoreSetting,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct StoreSetting {
    pub broadcast_interval: u64,
    pub broadcast_with_connection: bool,
    pub discovery: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub ipfs_setting: IpfsSetting,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            ipfs_setting: IpfsSetting {
                path: None,
                bootstrap: vec![
                    "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
                    "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
                    "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
                    "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
                    "/ip4/104.131.131.82/tcp/4001/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ",
                ]
                .iter()
                .filter_map(|s| Multiaddr::from_str(s).ok())
                .collect::<Vec<_>>(),
                listen_on: vec!["/ip4/0.0.0.0/tcp/0", "/ip6/::/tcp/0"]
                    .iter()
                    .filter_map(|s| Multiaddr::from_str(s).ok())
                    .collect::<Vec<_>>(),
                mdns: Mdns { enable: true },
                autonat: Autonat {
                    enable: false,
                    servers: vec![],
                },
                relay_client: RelayClient {
                    enable: false,
                    relay_address: None,
                },
                relay_server: RelayServer { enable: false },
                dcutr: Dcutr { enable: false },
                store_setting: StoreSetting {
                    broadcast_interval: 50,
                    discovery: false,
                    broadcast_with_connection: false,
                },
                ..Default::default()
            },
        }
    }
}

impl Config {
    pub fn development() -> Config {
        Config {
            ipfs_setting: IpfsSetting {
                path: None,
                listen_on: vec!["/ip4/127.0.0.1/tcp/0"]
                    .iter()
                    .filter_map(|s| Multiaddr::from_str(s).ok())
                    .collect::<Vec<_>>(),
                mdns: Mdns { enable: true },
                store_setting: StoreSetting {
                    broadcast_interval: 50,
                    discovery: false,
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }

    pub fn production() -> Config {
        Config {
            ipfs_setting: IpfsSetting {
                path: None,
                listen_on: vec!["/ip4/0.0.0.0/tcp/0", "/ip6/::/tcp/0"]
                    .iter()
                    .filter_map(|s| Multiaddr::from_str(s).ok())
                    .collect::<Vec<_>>(),
                mdns: Mdns { enable: true },
                autonat: Autonat {
                    enable: true,
                    servers: vec![],
                },
                relay_client: RelayClient {
                    enable: true,
                    relay_address: None,
                },
                dcutr: Dcutr { enable: true },
                store_setting: StoreSetting {
                    broadcast_interval: 100,
                    discovery: false,
                    ..Default::default()
                },
                temporary: false,
                ..Default::default()
            },
        }
    }
}

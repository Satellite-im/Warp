use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, path::PathBuf};

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
    pub mdns: Mdns,
    pub autonat: Autonat,
    pub relay_client: RelayClient,
    pub relay_server: RelayServer,
    pub dcutr: Dcutr,
    pub rendezvous: Rendezvous,
}

//TODO: section for friends and identity discovery
//      - In the case these are not set or unable to make a connection to fall back to using ipfs DHT
//        and performing a lookup of provided cid to make a connection. 
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub path: Option<PathBuf>,
    pub bootstrap: Vec<Multiaddr>,
    pub listen_on: Vec<Multiaddr>,
    pub ipfs_setting: IpfsSetting,
}

impl Default for Config {
    fn default() -> Self {
        Config {
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
            ipfs_setting: IpfsSetting {
                mdns: Mdns {
                    enable: false,
                },
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
                rendezvous: Rendezvous {
                    enable: false,
                    ..Default::default()
                },
            },
        }
    }
}

impl Config {
    pub fn development() -> Config {
        Self::default()
    }
}

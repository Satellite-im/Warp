use serde::{Deserialize, Serialize};

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
pub struct Relay {
    pub enable: bool,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct BehaviourConfig {
    pub mdns: Mdns,
    pub autonat: Autonat,
    pub relay: Relay,
    pub dcutr: Dcutr,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub bootstrap: Vec<(String, String)>,
    pub listen_on: String,
    pub behaviour: BehaviourConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bootstrap: vec![
                (
                    "/dnsaddr/bootstrap.libp2p.io",
                    "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
                ),
                (
                    "/dnsaddr/bootstrap.libp2p.io",
                    "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
                ),
                (
                    "/dnsaddr/bootstrap.libp2p.io",
                    "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
                ),
                (
                    "/dnsaddr/bootstrap.libp2p.io",
                    "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
                ),
            ]
            .iter()
            .map(|(p, a)| (p.to_string(), a.to_string()))
            .collect::<Vec<(String, String)>>(),
            listen_on: String::from("/ip4/0.0.0.0/tcp/4710"),
            behaviour: BehaviourConfig {
                mdns: Mdns {
                    enable: true,
                    enable_ipv6: true,
                },
                autonat: Autonat { enable: true },
                relay: Relay { enable: true },
                dcutr: Dcutr { enable: true },
            },
        }
    }
}

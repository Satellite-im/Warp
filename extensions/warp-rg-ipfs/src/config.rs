use ipfs::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Dcutr {
    pub enable: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Mdns {
    pub enable: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Autonat {
    pub enable: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<Multiaddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayClient {
    pub enable: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relay_address: Vec<Multiaddr>,
}

impl Default for RelayClient {
    fn default() -> Self {
        Self {
            enable: false,
            relay_address: vec![
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN/p2p-circuit",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa/p2p-circuit",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb/p2p-circuit",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt/p2p-circuit",
            ]
                    .iter()
                    .filter_map(|s| Multiaddr::from_str(s).ok())
                    .collect::<Vec<_>>(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RelayServer {
    pub enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rendezvous {
    pub enable: bool,
    #[serde(skip_serializing_if = "Multiaddr::is_empty")]
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IpfsSetting {
    pub mdns: Mdns,
    pub autonat: Autonat,
    pub relay_client: RelayClient,
    pub relay_server: RelayServer,
    pub dcutr: Dcutr,
    pub rendezvous: Rendezvous,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StoreSetting {
    pub broadcast_interval: u64,
    pub broadcast_with_connection: bool,
    pub discovery: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RgIpfsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bootstrap: Vec<Multiaddr>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub listen_on: Vec<Multiaddr>,
    pub ipfs_setting: IpfsSetting,
    pub store_setting: StoreSetting,
}

impl Default for RgIpfsConfig {
    fn default() -> Self {
        RgIpfsConfig {
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
                mdns: Mdns { enable: true },
                autonat: Autonat {
                    enable: false,
                    servers: vec![],
                },
                relay_client: RelayClient {
                    enable: false,
                    ..Default::default()
                },
                relay_server: RelayServer { enable: false },
                dcutr: Dcutr { enable: false },
                ..Default::default()
            },
            store_setting: StoreSetting {
                broadcast_interval: 100,
                discovery: true,
                broadcast_with_connection: false,
            },
        }
    }
}

impl RgIpfsConfig {
    pub fn development() -> RgIpfsConfig {
        RgIpfsConfig {
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                ..Default::default()
            },
            store_setting: StoreSetting {
                broadcast_interval: 100,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn testing() -> RgIpfsConfig {
        RgIpfsConfig {
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                relay_client: RelayClient {
                    enable: true,
                    ..Default::default()
                },
                dcutr: Dcutr { enable: true },
                autonat: Autonat {
                    enable: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            store_setting: StoreSetting {
                discovery: true,
                broadcast_interval: 100,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn production<P: AsRef<std::path::Path>>(path: P) -> RgIpfsConfig {
        RgIpfsConfig {
            path: Some(path.as_ref().to_path_buf()),
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                autonat: Autonat {
                    enable: true,
                    servers: vec![],
                },
                relay_client: RelayClient {
                    enable: true,
                    ..Default::default()
                },
                dcutr: Dcutr { enable: true },
                ..Default::default()
            },
            store_setting: StoreSetting {
                broadcast_interval: 100,
                discovery: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub mod ffi {
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::error::Error;
    use warp::ffi::FFIResult;
    use crate::RgIpfsConfig;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn rg_ipfs_config_from_file(
        file: *const c_char,
    ) -> FFIResult<RgIpfsConfig> {

        if file.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("file cannot be null")));
        }

        let file = CStr::from_ptr(file).to_string_lossy().to_string();

        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(e) => return FFIResult::err(Error::from(e))
        };

        let config = match serde_json::from_slice(&bytes) {
            Ok(config) => config,
            Err(e) => return FFIResult::err(Error::from(e))
        };

        FFIResult::ok(config)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn rg_ipfs_config_from_str(
        config: *const c_char,
    ) -> FFIResult<RgIpfsConfig> {

        if config.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let data = CStr::from_ptr(config).to_string_lossy().to_string();

        let config = match serde_json::from_str(&data) {
            Ok(config) => config,
            Err(e) => return FFIResult::err(Error::from(e))
        };

        FFIResult::ok(config)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn rg_ipfs_config_development() -> *mut RgIpfsConfig {
        Box::into_raw(Box::new(RgIpfsConfig::development()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn rg_ipfs_config_testing() -> *mut RgIpfsConfig {
        Box::into_raw(Box::new(RgIpfsConfig::testing()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn rg_ipfs_config_production(path: *const c_char) -> FFIResult<RgIpfsConfig> {
        if path.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let path = CStr::from_ptr(path).to_string_lossy().to_string();

        FFIResult::ok(RgIpfsConfig::production(path))
    }
}
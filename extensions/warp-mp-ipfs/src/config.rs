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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Swarm {
    dial_factor: u8,
    notify_buffer_size: usize,
    connection_buffer_size: usize,
    limit: Option<ConnectionLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionLimit {
    max_pending_incoming: u32,
    max_pending_outgoing: u32,
    max_established_incoming: u32,
    max_established_outgoing: u32,
    max_established: u32,
    max_established_per_peer: u32,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swarm: Option<Swarm>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StoreSetting {
    pub broadcast_interval: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery_name: Option<String>,
    pub discovery: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sync: Vec<Multiaddr>,
    pub sync_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpIpfsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bootstrap: Vec<Multiaddr>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub listen_on: Vec<Multiaddr>,
    pub ipfs_setting: IpfsSetting,
    pub store_setting: StoreSetting,
    pub debug: bool,
}

impl Default for MpIpfsConfig {
    fn default() -> Self {
        MpIpfsConfig {
            path: None,
            bootstrap: vec![
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
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
                ..Default::default()
            },
            store_setting: StoreSetting {
                broadcast_interval: 100,
                ..Default::default()
            },
            debug: false,
        }
    }
}

impl MpIpfsConfig {
    pub fn development() -> MpIpfsConfig {
        MpIpfsConfig {
            path: None,
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn testing() -> MpIpfsConfig {
        MpIpfsConfig {
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

    pub fn production<P: AsRef<std::path::Path>>(path: P) -> MpIpfsConfig {
        MpIpfsConfig {
            path: Some(path.as_ref().to_path_buf()),
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                autonat: Autonat {
                    enable: true,
                    ..Default::default()
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
    use crate::MpIpfsConfig;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::error::Error;
    use warp::ffi::{FFIResult, FFIResult_Null};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_from_file(
        file: *const c_char,
    ) -> FFIResult<MpIpfsConfig> {
        if file.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("file cannot be null")));
        }

        let file = CStr::from_ptr(file).to_string_lossy().to_string();

        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        let config = match serde_json::from_slice(&bytes) {
            Ok(config) => config,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        FFIResult::ok(config)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_to_file(
        config: *const MpIpfsConfig,
        file: *const c_char,
    ) -> FFIResult_Null {
        if config.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "config".into(),
            });
        }

        if file.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "file".into(),
            });
        }

        let file = CStr::from_ptr(file).to_string_lossy().to_string();

        let config = match serde_json::to_vec(&*config) {
            Ok(config) => config,
            Err(e) => return FFIResult_Null::err(Error::from(e)),
        };

        std::fs::write(file, &config).map_err(Error::from).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_from_str(
        config: *const c_char,
    ) -> FFIResult<MpIpfsConfig> {
        if config.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let data = CStr::from_ptr(config).to_string_lossy().to_string();

        let config = match serde_json::from_str(&data) {
            Ok(config) => config,
            Err(e) => return FFIResult::err(Error::from(e)),
        };

        FFIResult::ok(config)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_development() -> *mut MpIpfsConfig {
        Box::into_raw(Box::new(MpIpfsConfig::development()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_testing() -> *mut MpIpfsConfig {
        Box::into_raw(Box::new(MpIpfsConfig::testing()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_production(
        path: *const c_char,
    ) -> FFIResult<MpIpfsConfig> {
        if path.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let path = CStr::from_ptr(path).to_string_lossy().to_string();

        FFIResult::ok(MpIpfsConfig::production(path))
    }
}

use ipfs::Multiaddr;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bootstrap {
    #[default]
    Ipfs,
    Experimental,
    Custom(Vec<Multiaddr>),
    None,
}

impl Bootstrap {
    /// List of bootstrap multiaddr
    pub fn address(&self) -> Vec<Multiaddr> {
        match self {
            Bootstrap::Ipfs => vec![
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
            ]
            .iter()
            .filter_map(|s| Multiaddr::from_str(s).ok())
            .collect::<Vec<_>>(),
            Bootstrap::Experimental => vec!["/ip4/67.205.175.147/tcp/5000/p2p/12D3KooWDC7igsZ9Yaheip77ejALmjG6AZm2auuVmMDj1AkC2o7B"]
            .iter()
            .filter_map(|s| Multiaddr::from_str(s).ok())
            .collect::<Vec<_>>(),
            Bootstrap::Custom(address) => address.clone(),
            Bootstrap::None => vec![]
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Mdns {
    /// Enables mdns protocol in libp2p
    pub enable: bool,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RelayClient {
    /// Enables relay client in libp2p
    pub enable: bool,
    /// Enables DCUtR (requires relay to be enabled)
    pub dcutr: bool,
    /// Uses a single relay connection
    pub single: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// List of relays to use
    pub relay_address: Vec<Multiaddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Swarm {
    /// Concurrent dial factor
    pub dial_factor: u8,
    pub notify_buffer_size: usize,
    pub connection_buffer_size: usize,
    pub limit: Option<ConnectionLimit>,
}

impl Default for Swarm {
    fn default() -> Self {
        Self {
            dial_factor: 8, //Same dial factor as default for libp2p
            notify_buffer_size: 32,
            connection_buffer_size: 1024,
            limit: None,
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionLimit {
    pub max_pending_incoming: Option<u32>,
    pub max_pending_outgoing: Option<u32>,
    pub max_established_incoming: Option<u32>,
    pub max_established_outgoing: Option<u32>,
    pub max_established: Option<u32>,
    pub max_established_per_peer: Option<u32>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RelayServer {
    /// Used to enable the node as a relay server. Only should be enabled if the node isnt behind a NAT or could be connected to directly
    pub enable: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IpfsSetting {
    pub mdns: Mdns,
    pub relay_client: RelayClient,
    pub relay_server: RelayServer,
    pub swarm: Swarm,
    pub bootstrap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsIpfsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub bootstrap: Bootstrap,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub listen_on: Vec<Multiaddr>,
    pub ipfs_setting: IpfsSetting,
    pub max_storage_size: Option<usize>,
    pub max_file_size: Option<usize>,
    pub chunking: Option<usize>,
}

impl Default for FsIpfsConfig {
    fn default() -> Self {
        FsIpfsConfig {
            path: None,
            bootstrap: Bootstrap::Ipfs,
            listen_on: vec!["/ip4/0.0.0.0/tcp/0", "/ip6/::/tcp/0"]
                .iter()
                .filter_map(|s| Multiaddr::from_str(s).ok())
                .collect::<Vec<_>>(),
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                bootstrap: false,
                ..Default::default()
            },
            max_storage_size: Some(1024 * 1024 * 1024),
            max_file_size: Some(50 * 1024 * 1024),
            chunking: Some(1024 * 1024),
        }
    }
}

impl FsIpfsConfig {
    /// Default configuration for local development and writing test
    pub fn development() -> FsIpfsConfig {
        FsIpfsConfig::default()
    }

    /// Test configuration. Used for in-memory
    pub fn testing() -> FsIpfsConfig {
        FsIpfsConfig {
            bootstrap: Bootstrap::Ipfs,
            ipfs_setting: IpfsSetting {
                bootstrap: true,
                mdns: Mdns { enable: true },
                relay_client: RelayClient {
                    enable: true,
                    dcutr: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Minimial production configuration
    pub fn minimial_testing() -> FsIpfsConfig {
        FsIpfsConfig {
            bootstrap: Bootstrap::Ipfs,
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                bootstrap: false,
                relay_client: RelayClient {
                    enable: true,
                    dcutr: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Minimial production configuration
    pub fn minimial<P: AsRef<std::path::Path>>(path: P) -> FsIpfsConfig {
        FsIpfsConfig {
            bootstrap: Bootstrap::Ipfs,
            path: Some(path.as_ref().to_path_buf()),
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                bootstrap: false,
                relay_client: RelayClient {
                    enable: true,
                    dcutr: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Recommended production configuration
    pub fn production<P: AsRef<std::path::Path>>(path: P) -> FsIpfsConfig {
        FsIpfsConfig {
            bootstrap: Bootstrap::Ipfs,
            path: Some(path.as_ref().to_path_buf()),
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                bootstrap: true,
                relay_client: RelayClient {
                    enable: true,
                    dcutr: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<FsIpfsConfig> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        let config = serde_json::from_slice(&bytes)?;
        Ok(config)
    }

    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let path = path.as_ref();
        let bytes = serde_json::to_vec(self)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    pub fn from_string<S: AsRef<str>>(data: S) -> anyhow::Result<FsIpfsConfig> {
        let data = data.as_ref();
        let config = serde_json::from_str(data)?;
        Ok(config)
    }
}

pub mod ffi {
    use super::FsIpfsConfig;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::error::Error;
    use warp::ffi::{FFIResult, FFIResult_Null, FFIResult_String};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn fs_ipfs_config_from_file(
        file: *const c_char,
    ) -> FFIResult<FsIpfsConfig> {
        if file.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("file cannot be null")));
        }

        let file = CStr::from_ptr(file).to_string_lossy().to_string();

        FsIpfsConfig::from_file(file).map_err(Error::from).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn fs_ipfs_config_to_file(
        config: *const FsIpfsConfig,
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

        FsIpfsConfig::to_file(&*config, file)
            .map_err(Error::from)
            .into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn fs_ipfs_config_from_str(
        config: *const c_char,
    ) -> FFIResult<FsIpfsConfig> {
        if config.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let data = CStr::from_ptr(config).to_string_lossy().to_string();

        FsIpfsConfig::from_string(data).map_err(Error::from).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn fs_ipfs_config_to_str(
        config: *const FsIpfsConfig,
    ) -> FFIResult_String {
        if config.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        serde_json::to_string(&*config).map_err(Error::from).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn fs_ipfs_config_development() -> *mut FsIpfsConfig {
        Box::into_raw(Box::new(FsIpfsConfig::development()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn fs_ipfs_config_testing() -> *mut FsIpfsConfig {
        Box::into_raw(Box::new(FsIpfsConfig::testing()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn fs_ipfs_config_production(
        path: *const c_char,
    ) -> FFIResult<FsIpfsConfig> {
        if path.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let path = CStr::from_ptr(path).to_string_lossy().to_string();

        FFIResult::ok(FsIpfsConfig::production(path))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn fs_ipfs_config_minimial(
        path: *const c_char,
    ) -> FFIResult<FsIpfsConfig> {
        if path.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let path = CStr::from_ptr(path).to_string_lossy().to_string();

        FFIResult::ok(FsIpfsConfig::minimial(path))
    }
}

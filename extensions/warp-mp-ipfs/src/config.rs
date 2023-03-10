use ipfs::Multiaddr;
use rust_ipfs as ipfs;
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

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Discovery {
    /// Uses DHT PROVIDER to find and connect to peers using the same context
    Provider(Option<String>),
    /// Dials out to peers directly. Using this will only work with the DID til that connection is made
    Direct,
    /// Disables Discovery over DHT or Directly (which relays on direct connection via multiaddr)
    #[default]
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

impl ConnectionLimit {
    pub fn testing() -> Self {
        Self {
            max_pending_incoming: Some(10),
            max_pending_outgoing: Some(10),
            max_established_incoming: Some(32),
            max_established_outgoing: Some(32),
            max_established: None,
            max_established_per_peer: None,
        }
    }

    pub fn minimal() -> Self {
        Self {
            max_pending_incoming: Some(128),
            max_pending_outgoing: Some(128),
            max_established_incoming: Some(128),
            max_established_outgoing: Some(128),
            max_established: None,
            max_established_per_peer: None,
        }
    }

    pub fn recommended() -> Self {
        Self {
            max_pending_incoming: Some(512),
            max_pending_outgoing: Some(512),
            max_established_incoming: Some(512),
            max_established_outgoing: Some(512),
            max_established: None,
            max_established_per_peer: None,
        }
    }

    pub fn maximum() -> Self {
        Self {
            max_pending_incoming: Some(512),
            max_pending_outgoing: Some(512),
            max_established_incoming: Some(1024),
            max_established_outgoing: Some(1024),
            max_established: None,
            max_established_per_peer: None,
        }
    }

    pub fn unrestricted() -> Self {
        Self {
            max_pending_incoming: None,
            max_pending_outgoing: None,
            max_established_incoming: None,
            max_established_outgoing: None,
            max_established: None,
            max_established_per_peer: None,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RelayServer {
    /// Used to enable the node as a relay server. Only should be enabled if the node isnt behind a NAT or could be connected to directly
    pub enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pubsub {
    pub max_transmit_size: usize,
}

impl Default for Pubsub {
    fn default() -> Self {
        Self {
            max_transmit_size: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IpfsSetting {
    pub mdns: Mdns,
    pub relay_client: RelayClient,
    pub relay_server: RelayServer,
    pub pubsub: Pubsub,
    pub swarm: Swarm,
    pub bootstrap: bool,
    pub portmapping: bool,
    pub agent_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreSetting {
    /// Interval for broadcasting out identity or checking queue (queue will be moved into its own system in the future)
    pub broadcast_interval: u64,
    /// Discovery type
    pub discovery: Discovery,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// Placeholder for a offline agents to obtain information regarding one own identity
    pub sync: Vec<Multiaddr>,
    /// Interval to push or check node
    pub sync_interval: u64,
    /// Use objects directly rather than a cid
    pub override_ipld: bool,

    pub share_platform: bool,

    pub use_phonebook: bool,

    pub wait_on_response: Option<u64>,
}

impl Default for StoreSetting {
    fn default() -> Self {
        Self {
            broadcast_interval: 1000,
            discovery: Discovery::Provider(None),
            sync: Vec::new(),
            sync_interval: 100,
            override_ipld: true,
            share_platform: false,
            use_phonebook: true,
            wait_on_response: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpIpfsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub bootstrap: Bootstrap,
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
            store_setting: Default::default(),
            debug: false,
        }
    }
}

impl MpIpfsConfig {
    /// Default configuration for local development and writing test
    pub fn development() -> MpIpfsConfig {
        MpIpfsConfig::default()
    }

    /// Test configuration. Used for in-memory
    pub fn testing(experimental: bool) -> MpIpfsConfig {
        MpIpfsConfig {
            bootstrap: match experimental {
                true => Bootstrap::Experimental,
                false => Bootstrap::Ipfs,
            },
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
            store_setting: StoreSetting {
                discovery: Discovery::Provider(None),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Minimal production configuration
    pub fn minimal_testing() -> MpIpfsConfig {
        MpIpfsConfig {
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
            store_setting: StoreSetting {
                discovery: Discovery::None,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Minimal production configuration
    pub fn minimal<P: AsRef<std::path::Path>>(path: P) -> MpIpfsConfig {
        MpIpfsConfig {
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
            store_setting: StoreSetting {
                discovery: Discovery::None,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Recommended production configuration
    pub fn production<P: AsRef<std::path::Path>>(path: P, experimental: bool) -> MpIpfsConfig {
        MpIpfsConfig {
            bootstrap: match experimental {
                true => Bootstrap::Experimental,
                false => Bootstrap::Ipfs,
            },
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
            store_setting: StoreSetting {
                discovery: Discovery::Provider(None),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<MpIpfsConfig> {
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

    pub fn from_string<S: AsRef<str>>(data: S) -> anyhow::Result<MpIpfsConfig> {
        let data = data.as_ref();
        let config = serde_json::from_str(data)?;
        Ok(config)
    }
}

pub mod ffi {
    use crate::MpIpfsConfig;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use warp::error::Error;
    use warp::ffi::{FFIResult, FFIResult_Null, FFIResult_String};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_from_file(
        file: *const c_char,
    ) -> FFIResult<MpIpfsConfig> {
        if file.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("file cannot be null")));
        }

        let file = CStr::from_ptr(file).to_string_lossy().to_string();

        MpIpfsConfig::from_file(file).map_err(Error::from).into()
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

        MpIpfsConfig::to_file(&*config, file)
            .map_err(Error::from)
            .into()
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

        MpIpfsConfig::from_string(data).map_err(Error::from).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_to_str(
        config: *const MpIpfsConfig,
    ) -> FFIResult_String {
        if config.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        serde_json::to_string(&*config).map_err(Error::from).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_development() -> *mut MpIpfsConfig {
        Box::into_raw(Box::new(MpIpfsConfig::development()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_testing(experimental: bool) -> *mut MpIpfsConfig {
        Box::into_raw(Box::new(MpIpfsConfig::testing(experimental)))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_production(
        path: *const c_char,
        experimental: bool,
    ) -> FFIResult<MpIpfsConfig> {
        if path.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let path = CStr::from_ptr(path).to_string_lossy().to_string();

        FFIResult::ok(MpIpfsConfig::production(path, experimental))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mp_ipfs_config_minimal(
        path: *const c_char,
    ) -> FFIResult<MpIpfsConfig> {
        if path.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
        }

        let path = CStr::from_ptr(path).to_string_lossy().to_string();

        FFIResult::ok(MpIpfsConfig::minimal(path))
    }
}

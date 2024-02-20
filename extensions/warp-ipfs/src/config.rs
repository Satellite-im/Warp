use ipfs::Multiaddr;
use rust_ipfs as ipfs;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};
use warp::{constellation::file::FileType, multipass::identity::Identity};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bootstrap {
    #[default]
    Ipfs,
    Custom(Vec<Multiaddr>),
    None,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Discovery {
    Shuttle {
        addresses: Vec<Multiaddr>,
    },
    /// Uses to find and connect to peers using the same namespace
    Namespace {
        namespace: Option<String>,
        discovery_type: DiscoveryType,
    },
    /// Disables Discovery over DHT or Directly (which relays on direct connection via multiaddr)
    #[default]
    None,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiscoveryType {
    #[default]
    DHT,
    RzPoint {
        addresses: Vec<Multiaddr>,
    },
}

impl Bootstrap {
    /// List of bootstrap multiaddr
    pub fn address(&self) -> Vec<Multiaddr> {
        match self {
            Bootstrap::Ipfs => [
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
                "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
            ]
            .iter()
            .filter_map(|s| Multiaddr::from_str(s).ok())
            .collect::<Vec<_>>(),
            Bootstrap::Custom(address) => address.clone(),
            Bootstrap::None => vec![],
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Mdns {
    /// Enables mdns protocol in libp2p
    pub enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayClient {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// List of relays to use
    pub relay_address: Vec<Multiaddr>,
    pub background: bool,
    pub quorum: RelayQuorum,
}

#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RelayQuorum {
    First,
    N(u8),
    #[default]
    All,
}

impl Default for RelayClient {
    fn default() -> Self {
        Self {
            relay_address: vec![
                //NYC-1
                "/ip4/146.190.184.59/tcp/4001/p2p/12D3KooWCHWLQXTR2N6ukWM99pZYc4TM82VS7eVaDE4Ryk8ked8h".parse().unwrap(), 
                "/ip4/146.190.184.59/udp/4001/quic-v1/p2p/12D3KooWCHWLQXTR2N6ukWM99pZYc4TM82VS7eVaDE4Ryk8ked8h".parse().unwrap(), 
                //SF-1
                "/ip4/64.225.88.100/udp/4001/quic-v1/p2p/12D3KooWMfyuTCbehQYy68zPH6vpGUwg8raKbrS7pd3qZrG7bFuB".parse().unwrap(), 
                "/ip4/64.225.88.100/tcp/4001/p2p/12D3KooWMfyuTCbehQYy68zPH6vpGUwg8raKbrS7pd3qZrG7bFuB".parse().unwrap(), 
                //NYC-1-EXP
                "/ip4/24.199.86.91/udp/46315/quic-v1/p2p/12D3KooWQcyxuNXxpiM7xyoXRZC7Vhfbh2yCtRg272CerbpFkhE6".parse().unwrap(),
                "/ip4/24.199.86.91/tcp/46315/p2p/12D3KooWQcyxuNXxpiM7xyoXRZC7Vhfbh2yCtRg272CerbpFkhE6".parse().unwrap()
            ],
            background: true,
            quorum: Default::default()
        }
    }
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct Swarm {
//     /// Concurrent dial factor
//     pub dial_factor: u8,
//     pub notify_buffer_size: usize,
//     pub connection_buffer_size: usize,
//     pub limit: Option<ConnectionLimit>,
// }

// impl Default for Swarm {
//     fn default() -> Self {
//         Self {
//             dial_factor: 8, //Same dial factor as default for libp2p
//             notify_buffer_size: 32,
//             connection_buffer_size: 1024,
//             limit: None,
//         }
//     }
// }

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
    pub pubsub: Pubsub,
    pub bootstrap: bool,
    pub portmapping: bool,
    pub agent_version: Option<String>,
    /// Used for testing with a memory transport
    pub memory_transport: bool,
    pub dht_client: bool,
    pub disable_quic: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum UpdateEvents {
    #[default]
    /// Emit events for all identity updates
    Enabled,
    /// Emit events for identity updates from friends
    FriendsOnly,
    /// Disable events
    Disable,
}

pub type DefaultPfpFn = std::sync::Arc<
    dyn Fn(&Identity) -> Result<(Vec<u8>, FileType), std::io::Error> + Send + Sync + 'static,
>;

#[derive(Default, Clone, Serialize, Deserialize)]
pub enum StoreOffline {
    Remote,
    Local {
        path: PathBuf,
    },
    RemoteAndLocal {
        path: PathBuf,
    },
    #[default]
    None,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StoreSetting {
    /// Allow only interactions with friends
    /// Note: This is ignored when it comes to chating between group chat recipients
    pub with_friends: bool,
    /// Interval for broadcasting out identity (cannot be less than 3 minutes)
    /// Note:
    ///     - If `None`, this will be disabled
    ///     - Will default to 3 minutes if less than
    ///     - This may be removed in the future
    pub auto_push: Option<Duration>,
    /// Discovery type
    pub discovery: Discovery,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    /// Placeholder for a offline agents to obtain information regarding one own identity
    pub offline_agent: Vec<Multiaddr>,
    /// Export account on update
    pub store_offline: StoreOffline,

    /// Fetch data over bitswap instead of pubsub
    pub fetch_over_bitswap: bool,
    /// Enables sharing platform (Desktop, Mobile, Web) information to another user
    pub share_platform: bool,
    /// Emit event for when a friend comes online or offline
    pub emit_online_event: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Waits for a response from peer for a specific duration
    pub friend_request_response_duration: Option<Duration>,
    /// Options to allow emitting identity events to all or just friends
    pub update_events: UpdateEvents,
    /// Disable providing images for identities
    pub disable_images: bool,
    /// Enables spam check
    pub check_spam: bool,
    /// Announce to mesh network
    pub announce_to_mesh: bool,
    /// Function to call to provide data for a default profile picture if one is not apart of the identity
    #[serde(skip)]
    pub default_profile_picture: Option<DefaultPfpFn>,
}

impl std::fmt::Debug for StoreSetting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreSetting").finish()
    }
}

impl Default for StoreSetting {
    fn default() -> Self {
        Self {
            auto_push: None,
            discovery: Discovery::Namespace {
                namespace: None,
                discovery_type: Default::default(),
            },

            offline_agent: Vec::new(),
            store_offline: StoreOffline::None,

            fetch_over_bitswap: false,
            share_platform: false,
            friend_request_response_duration: None,
            emit_online_event: false,
            update_events: Default::default(),
            disable_images: false,
            check_spam: true,
            with_friends: false,
            default_profile_picture: None,
            announce_to_mesh: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub bootstrap: Bootstrap,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub listen_on: Vec<Multiaddr>,
    pub ipfs_setting: IpfsSetting,
    pub store_setting: StoreSetting,
    pub enable_relay: bool,
    pub save_phrase: bool,
    pub max_storage_size: Option<usize>,
    pub max_file_size: Option<usize>,
    pub thumbnail_size: (u32, u32),
    pub chunking: Option<usize>,
    pub thumbnail_exact_format: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            path: None,
            bootstrap: Bootstrap::Ipfs,
            listen_on: ["/ip4/0.0.0.0/tcp/0", "/ip4/0.0.0.0/udp/0/quic-v1"]
                .iter()
                .filter_map(|s| Multiaddr::from_str(s).ok())
                .collect::<Vec<_>>(),
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                bootstrap: false,
                ..Default::default()
            },
            store_setting: Default::default(),
            enable_relay: true,
            save_phrase: false,
            max_storage_size: Some(10 * 1024 * 1024 * 1024),
            max_file_size: Some(50 * 1024 * 1024),
            thumbnail_size: (128, 128),
            chunking: None,
            thumbnail_exact_format: true,
        }
    }
}

impl Config {
    /// Default configuration for local development and writing test
    pub fn development() -> Config {
        Config::default()
    }

    /// Test configuration. Used for in-memory
    pub fn testing() -> Config {
        Config {
            bootstrap: Bootstrap::Ipfs,
            ipfs_setting: IpfsSetting {
                bootstrap: true,
                mdns: Mdns { enable: true },
                relay_client: RelayClient {
                    ..Default::default()
                },
                ..Default::default()
            },
            store_setting: StoreSetting {
                discovery: Discovery::Namespace {
                    namespace: None,
                    discovery_type: Default::default(),
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Minimal production configuration
    pub fn minimal<P: AsRef<std::path::Path>>(path: P) -> Config {
        Config {
            bootstrap: Bootstrap::Ipfs,
            path: Some(path.as_ref().to_path_buf()),
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                bootstrap: false,
                relay_client: RelayClient {
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
    pub fn production<P: AsRef<std::path::Path>>(path: P) -> Config {
        Config {
            bootstrap: Bootstrap::Ipfs,
            path: Some(path.as_ref().to_path_buf()),
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                bootstrap: true,
                relay_client: RelayClient {
                    ..Default::default()
                },
                ..Default::default()
            },
            store_setting: StoreSetting {
                discovery: Discovery::Namespace {
                    namespace: None,
                    discovery_type: Default::default(),
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
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

    pub fn from_string<S: AsRef<str>>(data: S) -> anyhow::Result<Config> {
        let data = data.as_ref();
        let config = serde_json::from_str(data)?;
        Ok(config)
    }
}

// pub mod ffi {
//     use crate::config::Config;

//     use std::ffi::CStr;
//     use std::os::raw::c_char;
//     use warp::error::Error;
//     use warp::ffi::{FFIResult, FFIResult_Null, FFIResult_String};

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn mp_ipfs_config_from_file(
//         file: *const c_char,
//     ) -> FFIResult<Config> {
//         if file.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("file cannot be null")));
//         }

//         let file = CStr::from_ptr(file).to_string_lossy().to_string();

//         Config::from_file(file).map_err(Error::from).into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn mp_ipfs_config_to_file(
//         config: *const Config,
//         file: *const c_char,
//     ) -> FFIResult_Null {
//         if config.is_null() {
//             return FFIResult_Null::err(Error::NullPointerContext {
//                 pointer: "config".into(),
//             });
//         }

//         if file.is_null() {
//             return FFIResult_Null::err(Error::NullPointerContext {
//                 pointer: "file".into(),
//             });
//         }

//         let file = CStr::from_ptr(file).to_string_lossy().to_string();

//         Config::to_file(&*config, file)
//             .map_err(Error::from)
//             .into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn mp_ipfs_config_from_str(
//         config: *const c_char,
//     ) -> FFIResult<Config> {
//         if config.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
//         }

//         let data = CStr::from_ptr(config).to_string_lossy().to_string();

//         Config::from_string(data).map_err(Error::from).into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn mp_ipfs_config_to_str(
//         config: *const Config,
//     ) -> FFIResult_String {
//         if config.is_null() {
//             return FFIResult_String::err(Error::Any(anyhow::anyhow!("config cannot be null")));
//         }

//         serde_json::to_string(&*config).map_err(Error::from).into()
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn mp_ipfs_config_development() -> *mut Config {
//         Box::into_raw(Box::new(Config::development()))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn mp_ipfs_config_testing(experimental: bool) -> *mut Config {
//         Box::into_raw(Box::new(Config::testing(experimental)))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn mp_ipfs_config_production(
//         path: *const c_char,
//         experimental: bool,
//     ) -> FFIResult<Config> {
//         if path.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
//         }

//         let path = CStr::from_ptr(path).to_string_lossy().to_string();

//         FFIResult::ok(Config::production(path, experimental))
//     }

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn mp_ipfs_config_minimal(
//         path: *const c_char,
//     ) -> FFIResult<Config> {
//         if path.is_null() {
//             return FFIResult::err(Error::Any(anyhow::anyhow!("config cannot be null")));
//         }

//         let path = CStr::from_ptr(path).to_string_lossy().to_string();

//         FFIResult::ok(Config::minimal(path))
//     }
// }

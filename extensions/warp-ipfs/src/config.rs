use ipfs::{Multiaddr, Protocol};
use rust_ipfs as ipfs;
use std::{path::PathBuf, str::FromStr, time::Duration};
use warp::{constellation::file::FileType, multipass::identity::Identity};

#[derive(Default, Debug, Clone)]
pub enum Bootstrap {
    #[default]
    Ipfs,
    Custom(Vec<Multiaddr>),
    None,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
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

#[derive(Debug, Default, Clone)]
pub struct Mdns {
    /// Enables mdns protocol in libp2p
    pub enable: bool,
}

#[derive(Debug, Clone)]
pub struct RelayClient {
    /// List of relays to use
    pub relay_address: Vec<Multiaddr>,
    pub background: bool,
    pub quorum: RelayQuorum,
}

#[derive(Default, Debug, Clone, Copy)]
pub enum RelayQuorum {
    First,
    N(u8),
    #[default]
    All,
}

impl Default for RelayClient {
    fn default() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
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
            // Relays that are meant to be used from a web standpoint.
            // Note: webrtc addresses are prone to change due an upstream issue and shouldnt be relied on for primary connections
            #[cfg(target_arch="wasm32")]
            relay_address: vec![
                //NYC-1
                "/ip4/146.190.184.59/tcp/4001/wss/p2p/12D3KooWCHWLQXTR2N6ukWM99pZYc4TM82VS7eVaDE4Ryk8ked8h".parse().unwrap(),
                "/ip4/146.190.184.59/udp/4002/webrtc-direct/certhash/uEiC7m8m2pxf_DHr488akg-wSAxsa-f2agH5zc2nE70vx_g/p2p/12D3KooWCHWLQXTR2N6ukWM99pZYc4TM82VS7eVaDE4Ryk8ked8h".parse().unwrap(),
                //SF-1
                "/ip4/64.225.88.100/tcp/4001/wss/p2p/12D3KooWMfyuTCbehQYy68zPH6vpGUwg8raKbrS7pd3qZrG7bFuB".parse().unwrap(),
                "/ip4/64.225.88.100/udp/4002/webrtc-direct/certhash/uEiD25LqH8FlAgimcIY4XB1QiHHROlCYn7WJIukuRMe3tfQ/p2p/12D3KooWMfyuTCbehQYy68zPH6vpGUwg8raKbrS7pd3qZrG7bFuB".parse().unwrap(),
                //NYC-1-EXP
                "/ip4/24.199.86.91/tcp/46315/wss/p2p/12D3KooWQcyxuNXxpiM7xyoXRZC7Vhfbh2yCtRg272CerbpFkhE6".parse().unwrap(),
                "/ip4/24.199.86.91/udp/4002/webrtc-direct/certhash/uEiCZ8YAx_IZ7_x5dKltFESWHe4TUg8_gYpla6tmWR9jlfw/p2p/12D3KooWQcyxuNXxpiM7xyoXRZC7Vhfbh2yCtRg272CerbpFkhE6".parse().unwrap()
            ],
            background: true,
            quorum: Default::default()
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct IpfsSetting {
    pub mdns: Mdns,
    pub relay_client: RelayClient,
    pub bootstrap: bool,
    pub portmapping: bool,
    pub agent_version: Option<String>,
    /// Used for testing with a memory transport
    pub memory_transport: bool,
    pub dht_client: bool,
}

pub type DefaultPfpFn = std::sync::Arc<
    dyn Fn(&Identity) -> Result<(Vec<u8>, FileType), std::io::Error> + Send + Sync + 'static,
>;

#[derive(Clone)]
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

    /// Fetch data over bitswap instead of pubsub
    pub fetch_over_bitswap: bool,
    /// Enables sharing platform (Desktop, Mobile, Web) information to another user
    pub share_platform: bool,
    /// Waits for a response from peer for a specific duration
    pub friend_request_response_duration: Option<Duration>,
    /// Disable providing images for identities
    pub disable_images: bool,
    /// Announce to mesh network
    pub announce_to_mesh: bool,
    /// Function to call to provide data for a default profile picture if one is not apart of the identity
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
            fetch_over_bitswap: false,
            share_platform: false,
            friend_request_response_duration: None,
            disable_images: false,
            with_friends: false,
            default_profile_picture: None,
            announce_to_mesh: false,
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub struct Config {
    path: Option<PathBuf>,
    bootstrap: Bootstrap,
    listen_on: Vec<Multiaddr>,
    ipfs_setting: IpfsSetting,
    store_setting: StoreSetting,
    enable_relay: bool,
    save_phrase: bool,
    max_storage_size: Option<usize>,
    max_file_size: Option<usize>,
    thumbnail_size: (u32, u32),
    thumbnail_exact_format: bool,
}

impl Config {
    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    pub fn bootstrap(&self) -> &Bootstrap {
        &self.bootstrap
    }

    pub fn listen_on(&self) -> &[Multiaddr] {
        &self.listen_on
    }

    pub fn ipfs_setting(&self) -> &IpfsSetting {
        &self.ipfs_setting
    }

    pub fn store_setting(&self) -> &StoreSetting {
        &self.store_setting
    }

    pub fn enable_relay(&self) -> bool {
        self.enable_relay
    }

    pub fn save_phrase(&self) -> bool {
        self.save_phrase
    }

    pub fn max_storage_size(&self) -> Option<usize> {
        self.max_storage_size
    }

    pub fn max_file_size(&self) -> Option<usize> {
        self.max_file_size
    }

    pub fn thumbnail_size(&self) -> (u32, u32) {
        self.thumbnail_size
    }

    pub fn thumbnail_exact_format(&self) -> bool {
        self.thumbnail_exact_format
    }
}

impl Config {
    pub fn path_mut(&mut self) -> &mut Option<PathBuf> {
        &mut self.path
    }

    pub fn bootstrap_mut(&mut self) -> &mut Bootstrap {
        &mut self.bootstrap
    }

    pub fn listen_on_mut(&mut self) -> &mut Vec<Multiaddr> {
        &mut self.listen_on
    }

    pub fn ipfs_setting_mut(&mut self) -> &mut IpfsSetting {
        &mut self.ipfs_setting
    }

    pub fn store_setting_mut(&mut self) -> &mut StoreSetting {
        &mut self.store_setting
    }

    pub fn enable_relay_mut(&mut self) -> &mut bool {
        &mut self.enable_relay
    }

    pub fn save_phrase_mut(&mut self) -> &mut bool {
        &mut self.save_phrase
    }

    pub fn max_storage_size_mut(&mut self) -> &mut Option<usize> {
        &mut self.max_storage_size
    }

    pub fn max_file_size_mut(&mut self) -> &mut Option<usize> {
        &mut self.max_file_size
    }

    pub fn thumbnail_size_mut(&mut self) -> &mut (u32, u32) {
        &mut self.thumbnail_size
    }

    pub fn thumbnail_exact_format_mut(&mut self) -> &mut bool {
        &mut self.thumbnail_exact_format
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            path: None,
            bootstrap: Bootstrap::Ipfs,
            #[cfg(not(target_arch = "wasm32"))]
            listen_on: ["/ip4/0.0.0.0/tcp/0", "/ip4/0.0.0.0/udp/0/quic-v1"]
                .iter()
                .filter_map(|s| Multiaddr::from_str(s).ok())
                .collect::<Vec<_>>(),
            #[cfg(target_arch = "wasm32")]
            listen_on: vec![],
            ipfs_setting: IpfsSetting {
                mdns: Mdns { enable: true },
                bootstrap: false,
                ..Default::default()
            },
            store_setting: Default::default(),
            enable_relay: true,
            save_phrase: false,
            #[cfg(not(target_arch = "wasm32"))]
            max_storage_size: Some(10 * 1024 * 1024 * 1024),
            #[cfg(target_arch = "wasm32")]
            max_storage_size: Some(50 * 1024 * 1024),
            max_file_size: Some(50 * 1024 * 1024),
            thumbnail_size: (128, 128),
            thumbnail_exact_format: true,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
impl Config {
    /// Default configuration for local development and writing test
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
    pub fn development() -> Config {
        Config::default()
    }

    /// Test configuration. Used for in-memory
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
    pub fn testing() -> Config {
        Config {
            bootstrap: Bootstrap::Ipfs,
            listen_on: vec![Multiaddr::empty().with(Protocol::Memory(0))],
            ipfs_setting: IpfsSetting {
                bootstrap: true,
                mdns: Mdns { enable: true },
                relay_client: RelayClient {
                    ..Default::default()
                },
                memory_transport: true,
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

    /// Minimal testing configuration. Used for in-memory
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
    pub fn minimal_testing() -> Config {
        Config {
            bootstrap: Bootstrap::None,
            listen_on: vec![Multiaddr::empty().with(Protocol::Memory(0))],
            ipfs_setting: IpfsSetting {
                bootstrap: true,
                mdns: Mdns { enable: true },
                relay_client: RelayClient {
                    ..Default::default()
                },
                memory_transport: true,
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
    #[cfg(not(target_arch = "wasm32"))]
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
    #[cfg(not(target_arch = "wasm32"))]
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
}

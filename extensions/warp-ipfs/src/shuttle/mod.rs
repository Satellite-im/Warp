use rust_ipfs::PeerId;

pub mod gateway;
pub mod identity;
pub mod message;

#[cfg(not(target_arch = "wasm32"))]
pub mod server;
#[cfg(not(target_arch = "wasm32"))]
pub mod store;
#[cfg(not(target_arch = "wasm32"))]
pub mod subscription_stream;

pub enum ShuttleNodeQuorum {
    Primary,
    Seconary,
    Select(PeerId),
    All,
}

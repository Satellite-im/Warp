#[cfg(not(target_arch = "wasm32"))]
pub mod constellation;
pub mod crypto;
pub mod data;
#[cfg(not(target_arch = "wasm32"))]
pub mod error;
#[cfg(not(target_arch = "wasm32"))]
pub mod hooks;
pub mod module;
#[cfg(not(target_arch = "wasm32"))]
pub mod multipass;
#[cfg(not(target_arch = "wasm32"))]
pub mod pocket_dimension;
#[cfg(not(target_arch = "wasm32"))]
pub mod raygun;
#[cfg(not(target_arch = "wasm32"))]
pub mod solana;
pub mod tesseract;

#[cfg(not(target_arch = "wasm32"))]
pub mod common;

pub trait Extension {
    /// Returns an id of the extension. Should be the crate name (eg in a `warp-module-ext` format)
    fn id(&self) -> String;

    /// Returns the name of an extension
    fn name(&self) -> String;

    /// Returns the description of the extension
    fn description(&self) -> String {
        format!(
            "{} is an extension that is designed to be used for {}",
            self.name(),
            self.module()
        )
    }

    /// Returns the module type the extension is meant to be used for
    fn module(&self) -> crate::module::Module;
}

// cfg_if! {
//     if #[cfg(feature = "constellation")] {
//         pub use warp_constellation as constellation;
//         pub use warp_fs_memory as fs_memory;
//         pub use warp_fs_storj as fs_storj;
//         pub use warp_fs_ipfs as fs_ipfs;
//     }
// }
// cfg_if! {
//     if #[cfg(feature = "pocket-dimension")] {
//         pub use warp_pocket_dimension as pocket_dimension;
//         pub use warp_pd_stretto as pd_stretto;
//         pub use warp_pd_flatfile as pd_flatfile;
//     }
// }
//
// cfg_if! {
//     if #[cfg(feature = "multipass")] {
//         pub use warp_multipass as multipass;
//         pub use warp_mp_solana as mp_solana;
//     }
// }
//
// cfg_if! {
//     if #[cfg(feature = "raygun")] {
//         pub use warp_raygun as raygun;
//     }
// }

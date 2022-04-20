pub use cfg_if::cfg_if;
#[cfg(not(target_arch = "wasm32"))]
cfg_if! {
    if #[cfg(feature = "indirect")] {
        pub mod indirect;
        pub use indirect::Tesseract;
    } else if #[cfg(feature = "direct")] {
        pub mod direct;
        pub use direct::Tesseract;
    }
}

#[cfg(target_arch = "wasm32")]
pub mod wasm;

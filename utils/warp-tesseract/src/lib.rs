#[cfg(not(target_arch = "wasm32"))]
pub mod tesseract;
#[cfg(not(target_arch = "wasm32"))]
pub use tesseract::Tesseract;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::Tesseract;

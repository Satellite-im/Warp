#[cfg(not(target_arch = "wasm32"))]
pub use tokio::task::spawn;

#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_futures::spawn_local as spawn;

#[cfg(not(target_arch = "wasm32"))]
pub use tokio::task::JoinHandle;

#[cfg(target_arch = "wasm32")]
pub struct JoinHandle<T>(std::marker::PhantomData<T>);

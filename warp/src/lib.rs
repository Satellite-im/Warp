pub mod sync {
    pub use parking_lot::{Mutex, MutexGuard};
    pub use std::sync::Arc;
}

pub mod constellation;
pub mod crypto;
pub mod data;
pub mod error;
pub mod hooks;
pub mod module;
pub mod multipass;
pub mod pocket_dimension;
pub mod raygun;
pub mod tesseract;

#[cfg(not(target_arch = "wasm32"))]
static RUNTIME: once_cell::sync::Lazy<tokio::runtime::Runtime> = once_cell::sync::Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

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

#[cfg(all(target_arch = "wasm32", feature = "wasm_debug"))]
use wasm_bindgen::prelude::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm_debug"))]
#[wasm_bindgen(start)]
pub fn initialize() {
    // Any other code that would be used in a manner for initialization can be put here
    // assuming it would compile and be compatible with WASM

    // Allows us to show detailed error messages in console
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn runtime_handle() -> tokio::runtime::Handle {
    RUNTIME.handle().clone()
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    warp_derive::construct_ffi!();
    // #[allow(unused_macros)]
    // macro_rules! create_ffiarray_functions {
    //     ($code:expr) => {
    //         paste::item! {
    //             #[no_mangle]
    //             pub extern "C" fn [<ffiarray_ $code:lower _get>](ptr: *const crate::ffi::FFIArray<$code>, index: usize) -> *mut $code {
    //                 unsafe {
    //                     if ptr.is_null() {
    //                         return std::ptr::null();
    //                     }
    //                     let array = &*(ptr);
    //                     match array.get(index).cloned() {
    //                         Some(data) => Box::into_raw(Box::new(data)) as *mut $code,
    //                         None => std::ptr::null(),
    //                     }
    //                 }
    //             }
    //
    //             #[no_mangle]
    //             pub unsafe extern "C" fn [<ffiarray_ $code:lower _length>](ptr: *const crate::ffi::FFIArray<$code>) -> usize {
    //                 if ptr.is_null() {
    //                     return 0;
    //                 }
    //                 let array = &*(ptr);
    //                 array.length()
    //             }
    //         }
    //     };
    // }
    //
    // #[allow(unused_imports)]
    // pub(crate) use create_ffiarray_functions;
    //
    // pub struct FFIArray<T> {
    //     value: Vec<T>,
    // }
    //
    // impl<T> FFIArray<T> {
    //     pub fn new(value: Vec<T>) -> FFIArray<T> {
    //         FFIArray { value }
    //     }
    //
    //     pub fn get(&self, index: usize) -> Option<&T> {
    //         self.value.get(index)
    //     }
    //     pub fn length(&self) -> usize {
    //         self.value.len()
    //     }
    // }
}

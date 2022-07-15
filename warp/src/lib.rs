pub mod sync {
    pub use parking_lot::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
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

/// Used to downcast a specific type from an extension to share to another
pub trait SingleHandle {
    fn handle(&self) -> Result<Box<dyn core::any::Any>, error::Error> {
        Err(error::Error::Unimplemented)
    }
}

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
pub fn async_on_block<F: futures::Future>(fut: F) -> std::io::Result<F::Output> {
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle,
        Err(_) => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .handle()
            .clone(),
    };
    Ok(handle.block_on(fut))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn async_on_block_uncheck<F: futures::Future>(fut: F) -> F::Output {
    async_on_block(fut).expect("Unexpected error")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn async_block_in_place<F: futures::Future>(fut: F) -> std::io::Result<F::Output> {
    tokio::task::block_in_place(|| async_on_block(fut))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn async_block_in_place_uncheck<F: futures::Future>(fut: F) -> F::Output {
    async_block_in_place(fut).expect("Unexpected error")
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    pub struct FFIArray<T> {
        value: Vec<T>,
    }

    impl<T> FFIArray<T> {
        pub fn new(value: Vec<T>) -> FFIArray<T> {
            FFIArray { value }
        }

        pub fn get(&self, index: usize) -> Option<&T> {
            self.value.get(index)
        }

        pub fn length(&self) -> usize {
            self.value.len()
        }
    }

    #[repr(C)]
    pub struct FFIVec<T> {
        pub ptr: *mut T,
        pub len: usize,
        pub cap: usize,
    }

    impl<T> FFIVec<T> {
        pub fn from(vec: Vec<T>) -> Self {
            let mut vec = std::mem::ManuallyDrop::new(vec);
            let len = vec.len();
            let cap = vec.capacity();
            let ptr = vec.as_mut_ptr();
            Self { ptr, len, cap }
        }

        pub fn into_vec(self) -> Vec<T> {
            unsafe { Vec::from_raw_parts(self.ptr, self.len, self.cap) }
        }
    }

    #[repr(C)]
    pub struct FFIError {
        pub error_type: *mut std::os::raw::c_char,
        pub error_message: *mut std::os::raw::c_char,
    }

    impl FFIError {
        pub fn new(err: crate::error::Error) -> Self {
            let error_message = std::ffi::CString::new(err.to_string()).unwrap().into_raw();
            let error_type = std::ffi::CString::new(err.enum_to_string()).unwrap().into_raw();

            FFIError {
                error_type,
                error_message,
            }
        }

        pub fn to_ptr(self) -> *mut FFIError {
            Box::into_raw(Box::new(self))
        }

        pub fn from_ptr(ptr: *mut FFIError) -> (String, String) {
            let error = unsafe { Box::from_raw(ptr) };
            let error_type = match error.error_type.is_null() {
                false => unsafe { std::ffi::CString::from_raw(error.error_type).into_string().unwrap() },
                true => String::new()
            };
            let error_message = match error.error_message.is_null() {
                false => unsafe { std::ffi::CString::from_raw(error.error_message).into_string().unwrap() },
                true => String::new()
            };

            (error_type, error_message)
        }
    }

    #[repr(C)]
    pub struct FFIResult<T> {
        pub data: *mut T,
        pub error: *mut FFIError,
    }

    #[repr(C)]
    pub struct FFIResult_String {
        pub data: *mut std::os::raw::c_char,
        pub error: *mut FFIError,
    }

    #[repr(C)]
    pub struct FFIResult_Vec<T> {
        pub data: *mut FFIVec<T>,
        pub error: *mut FFIError,
    }

    impl From<Result<String, crate::error::Error>> for FFIResult_String {
        fn from(res: Result<String, crate::error::Error>) -> Self {
            match res {
                Ok(t) => {
                    let data = std::ffi::CString::new(t).unwrap().into_raw();
                    Self {
                        data,
                        error: std::ptr::null_mut(),
                    }
                }
                Err(err) => {
                    let error = FFIError::new(err).to_ptr();
                    Self {
                        data: std::ptr::null_mut(),
                        error,
                    }
                },
            }
        }
    }

    impl<T> From<Result<Vec<T>, crate::error::Error>> for FFIResult_Vec<T> {
        fn from(res: Result<Vec<T>, crate::error::Error>) -> Self {
            match res {
                Ok(t) => {
                    let data = Box::into_raw(Box::new(FFIVec::from(t)));
                    Self {
                        data,
                        error: std::ptr::null_mut(),
                    }
                }
                Err(err) => {
                    let error = FFIError::new(err).to_ptr();
                    Self {
                        data: std::ptr::null_mut(),
                        error,
                    }
                }
            }
        }
    }

    impl<T> From<Result<Vec<T>, crate::error::Error>> for FFIResult<FFIVec<T>> {
        fn from(res: Result<Vec<T>, crate::error::Error>) -> Self {
            match res {
                Ok(t) => {
                    let data = Box::into_raw(Box::new(FFIVec::from(t)));
                    Self {
                        data,
                        error: std::ptr::null_mut(),
                    }
                }
                Err(err) => Self::err(err),
            }
        }
    }

    impl From<Result<String, crate::error::Error>> for FFIResult<std::os::raw::c_char> {
        fn from(res: Result<String, crate::error::Error>) -> Self {
            match res {
                Ok(t) => {
                    let data = std::ffi::CString::new(t).unwrap().into_raw();
                    Self {
                        data,
                        error: std::ptr::null_mut(),
                    }
                }
                Err(err) => Self::err(err),
            }
        }
    }

    impl From<Result<(), crate::error::Error>> for FFIResult<std::os::raw::c_void> {
        fn from(res: Result<(), crate::error::Error>) -> Self {
            match res {
                Ok(_) => Self {
                    data: std::ptr::null_mut(),
                    error: std::ptr::null_mut(),
                },
                Err(err) => Self::err(err),
            }
        }
    }

    impl<T> FFIResult<T> {
        /// Convert a Result<T, warp::error::Error> into FFIResult
        pub fn import(result: std::result::Result<T, crate::error::Error>) -> Self {
            match result {
                Ok(t) => FFIResult::ok(t),
                Err(err) => FFIResult::err(err),
            }
        }

        /// Produce a successful FFIResult
        pub fn ok(data: T) -> Self {
            Self {
                data: Box::into_raw(Box::new(data)),
                error: std::ptr::null_mut(),
            }
        }

        /// Produce a error of FFIResult
        pub fn err(err: crate::error::Error) -> Self {
            let error = FFIError::new(err).to_ptr();
            Self {
                data: std::ptr::null_mut(),
                error,
            }
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn ffierror_free(ptr: *mut FFIError) {
        if ptr.is_null() {
            return;
        }

        let _ = FFIError::from_ptr(ptr);
    }
}

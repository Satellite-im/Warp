#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use std::{
        ffi::{CStr, CString},
        os::raw::c_char,
    };

    use crate::{error::Error, ffi::FFIResult};

    use super::DID;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn did_to_string(did_key: *const DID) -> *mut c_char {
        if did_key.is_null() {
            return std::ptr::null_mut();
        }

        let did = &*did_key;

        match CString::new(did.to_string()) {
            Ok(cstr) => cstr.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn did_from_string(did_key: *const c_char) -> FFIResult<DID> {
        if did_key.is_null() {
            return FFIResult::err(Error::from(anyhow::anyhow!("did_key is null")));
        }
        let did_str = CStr::from_ptr(did_key).to_string_lossy().to_string();
        did_str.try_into().into()
    }
}

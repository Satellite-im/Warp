#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::multipass::generator::generate_name;
    use std::ffi::CString;
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_generate_name() -> *mut c_char {
        let name = generate_name();
        match CString::new(name) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }
}

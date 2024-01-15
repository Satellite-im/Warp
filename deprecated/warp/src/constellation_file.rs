
#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::constellation::file::File;
    use std::ffi::CStr;
    #[allow(unused)]
    use std::ffi::{c_void, CString};
    #[allow(unused)]
    use std::os::raw::{c_char, c_int};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn file_new(name: *const c_char) -> *mut File {
        let name = match name.is_null() {
            true => "unused".to_string(),
            false => CStr::from_ptr(name).to_string_lossy().to_string(),
        };
        let file = Box::new(File::new(name.as_str()));
        Box::into_raw(file)
    }
}

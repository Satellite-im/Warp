#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use std::{
        ffi::{CStr, CString},
        os::raw::c_char,
    };

    use crate::{error::Error, ffi::FFIResult_Null, tesseract::Tesseract};

    use super::PhraseType;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn generate_mnemonic_phrase(phrase_type: PhraseType) -> *mut c_char {
        let mnemonic = super::generate_mnemonic_phrase(phrase_type);
        match CString::new(mnemonic.into_phrase()) {
            Ok(cstr) => cstr.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn mnemonic_into_tesseract(
        tesseract: *mut Tesseract,
        phrase: *const c_char,
        save: bool,
    ) -> FFIResult_Null {
        if tesseract.is_null() {
            return FFIResult_Null::err(Error::from(anyhow::anyhow!("Tesseract cannot be null")));
        }

        if phrase.is_null() {
            return FFIResult_Null::err(Error::from(anyhow::anyhow!("Phrase cannot be null")));
        }

        let phrase = CStr::from_ptr(phrase).to_string_lossy().to_string();

        super::mnemonic_into_tesseract(&mut *tesseract, &phrase, None, save, false).into()
    }
}
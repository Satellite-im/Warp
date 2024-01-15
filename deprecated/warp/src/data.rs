#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::data::{Data, DataType};
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_String};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_new(data: DataType, payload: *const c_char) -> FFIResult<Data> {
        if payload.is_null() {
            //TODO: Determine if providing a NULL should be allowed and converted into `()` type
            return FFIResult::err(Error::Any(anyhow::anyhow!("Payload is required")));
        }

        let payload_str = CStr::from_ptr(payload).to_string_lossy().to_string();

        let payload_value =
            match serde_json::from_str::<serde_json::Value>(&payload_str).map_err(Error::from) {
                Ok(v) => v,
                Err(e) => return FFIResult::err(e),
            };

        match Data::new(data, payload_value) {
            Ok(data) => FFIResult::ok(data),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_id(data: *const Data) -> *mut c_char {
        if data.is_null() {
            return std::ptr::null_mut();
        }

        let data = &*data;
        let id = data.id();

        match CString::new(id.to_string()) {
            Ok(inner) => inner.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_update_time(data: *mut Data) {
        if data.is_null() {
            return;
        }

        let data = &mut *data;
        data.update_time()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_set_version(data: *mut Data, version: u32) {
        if data.is_null() {
            return;
        }

        let data = &mut *data;
        data.set_version(version)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_set_data_type(data: *mut Data, data_type: DataType) {
        if data.is_null() {
            return;
        }

        let data = &mut *data;
        data.set_data_type(data_type)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_set_size(data: *mut Data, size: u64) {
        if data.is_null() {
            return;
        }

        let data = &mut *data;
        data.set_size(size)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_size(data: *const Data) -> u64 {
        if data.is_null() {
            return 0;
        }

        let data = &*data;
        data.size()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_timestamp(data: *const Data) -> i64 {
        if data.is_null() {
            return 0;
        }

        let data = &*data;
        data.timestamp()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_version(data: *const Data) -> u32 {
        if data.is_null() {
            return 0;
        }

        let data = &*data;
        data.version()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_type(data: *const Data) -> DataType {
        if data.is_null() {
            return DataType::Unknown;
        }

        let data = &*data;
        data.data_type()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn data_payload(data: *const Data) -> FFIResult_String {
        if data.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("Data is null")));
        }

        let data = &*data;

        //Since C does not know Data::payload<T>(), we convert this into a json object and
        //return the string
        let payload = match data.payload::<serde_json::Value>() {
            Ok(payload) => payload,
            Err(e) => return FFIResult_String::err(e),
        };

        FFIResult_String::from(serde_json::to_string(&payload).map_err(Error::from))
    }
}
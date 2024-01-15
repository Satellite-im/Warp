#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {

    use futures::StreamExt;

    use crate::constellation::directory::Directory;
    use crate::constellation::{ConstellationAdapter, ConstellationDataType};
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null, FFIResult_String, FFIVec};
    use crate::runtime_handle;
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use super::ConstellationProgressStream;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_select(
        ctx: *mut ConstellationAdapter,
        name: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Name cannot be null")));
        }

        let cname = CStr::from_ptr(name).to_string_lossy().to_string();

        let constellation = &mut *(ctx);
        constellation.select(&cname).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_go_back(
        ctx: *mut ConstellationAdapter,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let constellation = &mut *(ctx);
        constellation.go_back().into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_open_directory(
        ctx: *mut ConstellationAdapter,
        name: *const c_char,
    ) -> FFIResult<Directory> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if name.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Name cannot be null")));
        }

        let cname = CStr::from_ptr(name).to_string_lossy().to_string();

        let constellation = &mut *(ctx);
        match constellation.open_directory(&cname) {
            Ok(directory) => FFIResult::ok(directory),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_root_directory(
        ctx: *const ConstellationAdapter,
    ) -> *mut Directory {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let constellation = &*(ctx);
        let directory = constellation.root_directory();
        Box::into_raw(Box::new(directory))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_current_directory(
        ctx: *mut ConstellationAdapter,
    ) -> FFIResult<Directory> {
        if ctx.is_null() {
            return FFIResult::err(Error::NullPointerContext {
                pointer: "ctx".into(),
            });
        }
        let constellation = &*(ctx);
        constellation.current_directory().into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_put(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        local: *const c_char,
    ) -> FFIResult<ConstellationProgressStream> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Remote path cannot be null")));
        }

        if local.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Local path cannot be null")));
        }

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let local = CStr::from_ptr(local).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async { constellation.put(&remote, &local).await })
            .into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_progress_stream_next(
        ctx: *mut ConstellationProgressStream,
    ) -> FFIResult_String {
        if ctx.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let stream = &mut *(ctx);
        let rt = runtime_handle();
        match rt.block_on(stream.next()) {
            Some(event) => serde_json::to_string(&event).map_err(Error::from).into(),
            None => FFIResult_String::err(Error::Any(anyhow::anyhow!(
                "Error obtaining data from stream"
            ))),
        }
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_put_buffer(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        buffer: *const u8,
        buffer_size: usize,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Remote path cannot be null")));
        }

        if buffer.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Buffer cannot be null")));
        }

        let slice = std::slice::from_raw_parts(buffer, buffer_size);

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote);
        let rt = runtime_handle();

        rt.block_on(async move {
            constellation
                .put_buffer(&remote.to_string_lossy(), slice)
                .await
        })
        .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_get(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        local: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Remote path cannot be null")));
        }

        if local.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Local path cannot be null")));
        }

        let constellation = &*(ctx);

        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let local = CStr::from_ptr(local).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.get(&remote, &local).await })
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_get_buffer(
        ctx: *const ConstellationAdapter,
        remote: *const c_char,
    ) -> FFIResult<FFIVec<u8>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Remote cannot be null")));
        }

        let constellation = &*ctx;
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move {
            match constellation.get_buffer(&remote).await {
                Ok(temp_buf) => FFIResult::ok(FFIVec::from(temp_buf)),
                Err(e) => FFIResult::err(e),
            }
        })
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_remove(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        recursive: bool,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Remote cannot be null")));
        }

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.remove(&remote, recursive).await })
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_create_directory(
        ctx: *mut ConstellationAdapter,
        remote: *const c_char,
        recursive: bool,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if remote.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Remote path cannot be null")));
        }

        let constellation = &mut *(ctx);
        let remote = CStr::from_ptr(remote).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.create_directory(&remote, recursive).await })
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_move_item(
        ctx: *mut ConstellationAdapter,
        src: *const c_char,
        dst: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if src.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Source cannot be null")));
        }

        if dst.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Dest cannot be null")));
        }

        let constellation = &mut *(ctx);
        let src = CStr::from_ptr(src).to_string_lossy().to_string();
        let dst = CStr::from_ptr(dst).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.move_item(&src, &dst).await })
            .into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_rename(
        ctx: *mut ConstellationAdapter,
        path: *const c_char,
        name: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if path.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Path cannot be null")));
        }

        if name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Name cannot be null")));
        }

        let constellation = &mut *(ctx);
        let path = CStr::from_ptr(path).to_string_lossy().to_string();
        let name = CStr::from_ptr(name).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(constellation.rename(&path, &name)).into()
    }

    #[allow(clippy::await_holding_lock)]
    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_sync_ref(
        ctx: *mut ConstellationAdapter,
        src: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if src.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Source cannot be null")));
        }

        let constellation = &mut *(ctx);
        let src = CStr::from_ptr(src).to_string_lossy().to_string();
        let rt = runtime_handle();
        rt.block_on(async move { constellation.sync_ref(&src).await })
            .into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_export(
        ctx: *mut ConstellationAdapter,
        datatype: ConstellationDataType,
    ) -> FFIResult_String {
        if ctx.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let constellation = &*(ctx);

        FFIResult_String::from(constellation.export(datatype))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_import(
        ctx: *mut ConstellationAdapter,
        datatype: ConstellationDataType,
        data: *const c_char,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if data.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Data cannot be null")));
        }

        let constellation = &mut *(ctx);
        let data = CStr::from_ptr(data).to_string_lossy().to_string();
        constellation.import(datatype, data).into()
    }
}

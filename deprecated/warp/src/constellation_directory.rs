#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::constellation::directory::Directory;
    use crate::constellation::file::File;
    use crate::constellation::item::FFIVec_Item; //file::FFIVec_File,  directory::FFIVec_Directory};
    use crate::constellation::item::Item;
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null};
    use std::ffi::CStr;
    use std::ffi::CString;
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_new(name: *const c_char) -> *mut Directory {
        let name = match name.is_null() {
            true => "unused".to_string(),
            false => CStr::from_ptr(name).to_string_lossy().to_string(),
        };
        let directory = Box::new(Directory::new(name.as_str()));
        Box::into_raw(directory)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_add_item(
        dir_ptr: *mut Directory,
        item: *const Item,
    ) -> FFIResult_Null {
        if dir_ptr.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if item.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Item is null")));
        }

        let dir_ptr = &mut *(dir_ptr);

        let item = &*(item);

        dir_ptr.add_item(item.clone()).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_add_directory(
        dir_ptr: *mut Directory,
        directory: *const Directory,
    ) -> FFIResult_Null {
        if dir_ptr.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory pointer is null")));
        }

        if directory.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        let dir_ptr = &mut *(dir_ptr);

        let new_directory = &*(directory);

        dir_ptr.add_directory(new_directory.clone()).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_add_file(
        dir_ptr: *mut Directory,
        file: *const File,
    ) -> FFIResult_Null {
        if dir_ptr.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if file.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("File is null")));
        }

        let dir_ptr = &mut *dir_ptr;

        let new_file = &*file;

        dir_ptr.add_file(new_file.clone()).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_get_item_index(
        dir_ptr: *const Directory,
        name: *const c_char,
    ) -> FFIResult<usize> {
        if dir_ptr.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if name.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Name is null")));
        }

        let dir_ptr = &*dir_ptr;

        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        dir_ptr.get_item_index(&name).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_rename_item(
        dir_ptr: *mut Directory,
        current_name: *const c_char,
        new_name: *const c_char,
    ) -> FFIResult_Null {
        if dir_ptr.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if current_name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Current name is null")));
        }

        if new_name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("New name is null")));
        }

        let dir_ptr = &mut *dir_ptr;

        let current_name = CStr::from_ptr(current_name).to_string_lossy().to_string();
        let new_name = CStr::from_ptr(new_name).to_string_lossy().to_string();

        dir_ptr.rename_item(&current_name, &new_name).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_remove_item(
        dir_ptr: *mut Directory,
        name: *const c_char,
    ) -> FFIResult<Item> {
        if dir_ptr.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if name.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("name is null")));
        }

        let dir_ptr = &mut *dir_ptr;

        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        dir_ptr.remove_item(&name).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_has_item(
        ptr: *const Directory,
        item: *const c_char,
    ) -> bool {
        if ptr.is_null() {
            return false;
        }

        if item.is_null() {
            return false;
        }

        let directory = &*ptr;

        let item = CStr::from_ptr(item).to_string_lossy().to_string();

        directory.has_item(&item)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_get_items(ptr: *const Directory) -> *mut FFIVec_Item {
        if ptr.is_null() {
            return std::ptr::null_mut();
        }

        let directory = &*ptr;

        Box::into_raw(Box::new(directory.get_items().into()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_get_item(
        ptr: *const Directory,
        item: *const c_char,
    ) -> FFIResult<Item> {
        if ptr.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory is null")));
        }

        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Item is null")));
        }

        let directory = &*ptr;

        let item = CStr::from_ptr(item).to_string_lossy().to_string();

        directory.get_item(&item).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_remove_item_from_path(
        ptr: *mut Directory,
        directory: *const c_char,
        item: *const c_char,
    ) -> FFIResult<Item> {
        if ptr.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory cannot be null")));
        }

        if directory.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Directory path cannot be null")));
        }

        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Item cannot be null")));
        }

        let dir = &mut *ptr;

        let directory = CStr::from_ptr(directory).to_string_lossy().to_string();
        let item = CStr::from_ptr(item).to_string_lossy().to_string();

        dir.remove_item_from_path(&directory, &item).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_move_item_to(
        ptr: *mut Directory,
        src: *const c_char,
        dst: *const c_char,
    ) -> FFIResult_Null {
        if ptr.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "ptr".into(),
            });
        }

        if src.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "src".into(),
            });
        }

        if dst.is_null() {
            return FFIResult_Null::err(Error::NullPointerContext {
                pointer: "dst".into(),
            });
        }

        let dir = &mut *ptr;

        let src = CStr::from_ptr(src).to_string_lossy().to_string();
        let dst = CStr::from_ptr(dst).to_string_lossy().to_string();

        dir.move_item_to(&src, &dst).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_id(dir: *const Directory) -> *mut c_char {
        if dir.is_null() {
            return std::ptr::null_mut();
        }

        let dir = &*dir;

        match CString::new(dir.id().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_name(dir: *const Directory) -> *mut c_char {
        if dir.is_null() {
            return std::ptr::null_mut();
        }

        let dir: &Directory = &*dir;

        match CString::new(dir.name()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_set_name(dir: *const Directory, desc: *const c_char) {
        if dir.is_null() {
            return;
        }

        let data = CStr::from_ptr(desc).to_string_lossy().to_string();
        Directory::set_name(&*dir, &data)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_description(dir: *const Directory) -> *mut c_char {
        if dir.is_null() {
            return std::ptr::null_mut();
        }

        let dir: &Directory = &*dir;

        match CString::new(dir.description()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_set_description(dir: *const Directory, desc: *const c_char) {
        if dir.is_null() {
            return;
        }

        let data = CStr::from_ptr(desc).to_string_lossy().to_string();
        Directory::set_description(&*dir, &data)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_size(dir: *const Directory) -> usize {
        if dir.is_null() {
            return 0;
        }

        let dir: &Directory = &*dir;

        dir.size()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_creation(dir: *const Directory) -> i64 {
        if dir.is_null() {
            return 0;
        }

        let dir: &Directory = &*dir;

        dir.creation().timestamp()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_modified(dir: *const Directory) -> i64 {
        if dir.is_null() {
            return 0;
        }

        let dir: &Directory = &*dir;

        dir.modified().timestamp()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_thumbnail(dir: *const Directory) -> *mut c_char {
        if dir.is_null() {
            return core::ptr::null_mut();
        }

        match CString::new(Directory::thumbnail(&*dir)) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_set_thumbnail(
        dir: *const Directory,
        thumbnail: *const c_char,
    ) {
        if dir.is_null() {
            return;
        }

        if thumbnail.is_null() {
            return;
        }

        let thumbnail = CStr::from_ptr(thumbnail).to_string_lossy().to_string();

        Directory::set_thumbnail(&*dir, thumbnail.as_bytes())
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_favorite(dir: *const Directory) -> bool {
        if dir.is_null() {
            return false;
        }

        Directory::favorite(&*dir)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_set_favorite(dir: *const Directory, fav: bool) {
        if dir.is_null() {
            return;
        }

        Directory::set_favorite(&*dir, fav)
    }
}

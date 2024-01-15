#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {

    use crate::constellation::directory::Directory;
    use crate::constellation::file::File;
    use crate::constellation::Item;
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use super::ItemType;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn directory_into_item(directory: *const Directory) -> *mut Item {
        if directory.is_null() {
            return std::ptr::null_mut();
        }
        let directory = &*directory;
        let item = Box::new(Item::new_directory(directory.clone()));
        Box::into_raw(item)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn file_into_item(file: *const File) -> *mut Item {
        if file.is_null() {
            return std::ptr::null_mut();
        }
        let file = &*file;
        let item = Box::new(Item::new_file(file.clone()));
        Box::into_raw(item)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_into_directory(item: *const Item) -> FFIResult<Directory> {
        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }
        let item = &*(item);
        FFIResult::import(item.get_directory())
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_into_file(item: *const Item) -> FFIResult<File> {
        if item.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }
        let item = &*(item);
        FFIResult::import(item.get_file())
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_id(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.name()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_creation(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.creation().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_modified(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.modified().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_name(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.name()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_description(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }
        let item = &*(item);
        match CString::new(item.description()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_type(item: *const Item) -> ItemType {
        if item.is_null() {
            return ItemType::InvalidItem;
        }
        Item::item_type(&*item)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_size(item: *const Item) -> usize {
        if item.is_null() {
            return 0;
        }
        let item = &*(item);
        item.size()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_favorite(item: *const Item) -> bool {
        if item.is_null() {
            return false;
        }
        Item::favorite(&*item)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_favorite(item: *const Item, fav: bool) {
        if item.is_null() {
            return;
        }
        Item::set_favorite(&*item, fav)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_thumbnail(item: *const Item) -> *mut c_char {
        if item.is_null() {
            return std::ptr::null_mut();
        }

        match CString::new(Item::thumbnail(&*item)) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_thumbnail(item: *const Item, data: *const c_char) {
        if item.is_null() {
            return;
        }

        let thumbnail = CStr::from_ptr(data).to_string_lossy().to_string();
        Item::set_thumbnail(&*item, thumbnail.as_bytes())
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_rename(item: *mut Item, name: *const c_char) -> FFIResult_Null {
        if item.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        if name.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let item = &mut *(item);
        let name = CStr::from_ptr(name).to_string_lossy().to_string();
        item.rename(&name).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_is_directory(item: *const Item) -> bool {
        if item.is_null() {
            return false;
        }
        let item = &*(item);
        item.is_directory()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_is_file(item: *const Item) -> bool {
        if item.is_null() {
            return false;
        }
        let item = &*(item);
        item.is_file()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_description(item: *mut Item, desc: *const c_char) {
        if item.is_null() {
            return;
        }

        if desc.is_null() {
            return;
        }

        let item = &mut *(item);

        let desc = CStr::from_ptr(desc).to_string_lossy().to_string();

        item.set_description(&desc);
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn item_set_size(item: *mut Item, size: usize) -> FFIResult_Null {
        if item.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let item = &mut *(item);

        item.set_size(size).into()
    }
}

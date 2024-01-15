#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::crypto::DID;
    use crate::multipass::identity::{Identifier, Identity, IdentityUpdate};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use super::Relationship;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_username(identity: *const Identity) -> *mut c_char {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        match CString::new(identity.username()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_short_id(identity: *const Identity) -> *mut c_char {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;
        match CString::new(identity.short_id().to_string()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_did_key(identity: *const Identity) -> *mut DID {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        Box::into_raw(Box::new(identity.did_key()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_status_message(
        identity: *const Identity,
    ) -> *mut c_char {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        match identity.status_message() {
            Some(status) => match CString::new(status) {
                Ok(c) => c.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            None => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identifier_user_name(
        name: *const c_char,
    ) -> *mut Identifier {
        if name.is_null() {
            return std::ptr::null_mut();
        }

        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        Box::into_raw(Box::new(Identifier::user_name(&name)))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identifier_did_key(key: *const DID) -> *mut Identifier {
        if key.is_null() {
            return std::ptr::null_mut();
        }

        let key = &*key;

        Box::into_raw(Box::new(Identifier::did_key(key.clone())))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identifier_own() -> *mut Identifier {
        Box::into_raw(Box::new(Identifier::own()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_set_username(
        name: *const c_char,
    ) -> *mut IdentityUpdate {
        if name.is_null() {
            return std::ptr::null_mut();
        }

        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        Box::into_raw(Box::new(IdentityUpdate::Username(name)))
    }

    // #[allow(clippy::missing_safety_doc)]
    // #[no_mangle]
    // pub unsafe extern "C" fn multipass_identity_update_set_graphics_picture(
    //     name: *const c_char,
    // ) -> *mut IdentityUpdate {
    //     if name.is_null() {
    //         return std::ptr::null_mut();
    //     }

    //     let name = CStr::from_ptr(name).to_string_lossy().to_string();

    //     Box::into_raw(Box::new(IdentityUpdate::Picture(name)))
    // }

    // #[allow(clippy::missing_safety_doc)]
    // #[no_mangle]
    // pub unsafe extern "C" fn multipass_identity_update_set_graphics_banner(
    //     name: *const c_char,
    // ) -> *mut IdentityUpdate {
    //     if name.is_null() {
    //         return std::ptr::null_mut();
    //     }

    //     let name = CStr::from_ptr(name).to_string_lossy().to_string();

    //     Box::into_raw(Box::new(IdentityUpdate::Banner(name)))
    // }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_set_status_message(
        name: *const c_char,
    ) -> *mut IdentityUpdate {
        let update = if !name.is_null() {
            let name = CStr::from_ptr(name).to_string_lossy().to_string();
            IdentityUpdate::StatusMessage(Some(name))
        } else {
            IdentityUpdate::StatusMessage(None)
        };

        Box::into_raw(Box::new(update))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_username(
        update: *const IdentityUpdate,
    ) -> *mut c_char {
        if update.is_null() {
            return std::ptr::null_mut();
        }

        let update = &*update;

        match update {
            IdentityUpdate::Username(data) => match CString::new(data.clone()) {
                Ok(data) => data.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            _ => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_graphics_picture(
        update: *const IdentityUpdate,
    ) -> *mut c_char {
        if update.is_null() {
            return std::ptr::null_mut();
        }

        let update = &*update;

        match update {
            IdentityUpdate::Picture(data) => match CString::new(data.clone()) {
                Ok(data) => data.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            _ => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_graphics_banner(
        update: *const IdentityUpdate,
    ) -> *mut c_char {
        if update.is_null() {
            return std::ptr::null_mut();
        }

        let update = &*update;

        match update {
            IdentityUpdate::Banner(data) => match CString::new(data.clone()) {
                Ok(data) => data.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            _ => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_status_message(
        update: *const IdentityUpdate,
    ) -> *mut c_char {
        if update.is_null() {
            return std::ptr::null_mut();
        }

        let update = &*update;

        if let IdentityUpdate::StatusMessage(Some(inner)) = update {
            if let Ok(data) = CString::new(inner.clone()) {
                return data.into_raw();
            }
        }
        std::ptr::null_mut()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_relationship_friends(
        context: *const Relationship,
    ) -> bool {
        if context.is_null() {
            return false;
        }

        Relationship::friends(&*context)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_relationship_received_friend_request(
        context: *const Relationship,
    ) -> bool {
        if context.is_null() {
            return false;
        }

        Relationship::received_friend_request(&*context)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_relationship_sent_friend_request(
        context: *const Relationship,
    ) -> bool {
        if context.is_null() {
            return false;
        }

        Relationship::sent_friend_request(&*context)
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_relationship_blocked(
        context: *const Relationship,
    ) -> bool {
        if context.is_null() {
            return false;
        }

        Relationship::blocked(&*context)
    }
}

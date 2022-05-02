#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub mod generator;
pub mod identity;

use crate::Extension;
use identity::Identity;

#[cfg(not(target_arch = "wasm32"))]
type Result<T> = std::result::Result<T, crate::error::Error>;

#[cfg(target_arch = "wasm32")]
type Result<T> = std::result::Result<T, JsError>;

use crate::multipass::identity::{FriendRequest, Identifier, IdentityUpdate, PublicKey};

pub trait MultiPass: Extension + Friends + Sync + Send {
    fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<PublicKey>;

    fn get_identity(&self, id: Identifier) -> Result<Identity>;

    fn get_own_identity(&self) -> Result<Identity> {
        self.get_identity(Identifier::own())
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<()>;

    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<Vec<u8>>;

    fn refresh_cache(&mut self) -> Result<()>;
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct MultiPassTraitObject {
    object: Box<dyn MultiPass>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl MultiPassTraitObject {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(object: Box<dyn MultiPass>) -> Self {
        MultiPassTraitObject { object }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_inner(&self) -> &Box<dyn MultiPass> {
        &self.object
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn get_inner_mut(&mut self) -> &mut Box<dyn MultiPass> {
        &mut self.object
    }
}

// #[warp_common::async_trait::async_trait]
pub trait Friends: Sync + Send {
    /// Send friend request to corresponding public key
    fn send_request(&mut self, pubkey: PublicKey) -> Result<()>;

    /// Accept friend request from public key
    fn accept_request(&mut self, pubkey: PublicKey) -> Result<()>;

    /// Deny friend request from public key
    fn deny_request(&mut self, pubkey: PublicKey) -> Result<()>;

    /// Closing or retracting friend request
    fn close_request(&mut self, pubkey: PublicKey) -> Result<()>;

    /// List the incoming friend request
    fn list_incoming_request(&self) -> Result<Vec<FriendRequest>>;

    /// List the outgoing friend request
    fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>>;

    /// List all the friend request that been sent or received
    fn list_all_request(&self) -> Result<Vec<FriendRequest>>;

    /// Remove friend from contacts
    fn remove_friend(&mut self, pubkey: PublicKey) -> Result<()>;

    /// Block public key, rather it be a friend or not, from being able to send request to account public address
    fn block_key(&mut self, pubkey: PublicKey) -> Result<()>;

    /// List all friends and return a corresponding identity for each public key
    fn list_friends(&self) -> Result<Vec<Identity>>;

    /// Check to see if public key is friend of the account
    fn has_friend(&self, pubkey: PublicKey) -> Result<()>;

    /// Returns a diffie-hellman key that is found between one private key and another public key
    fn key_exchange(&self, identity: Identity) -> Result<Vec<u8>>;
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::multipass::{
        identity::{FriendRequest, Identifier, Identity, IdentityUpdate, PublicKey},
        MultiPassTraitObject,
    };
    use std::ffi::CStr;
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_create_identity(
        ctx: *mut MultiPassTraitObject,
        username: *const c_char,
        passphrase: *const c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        let username = match username.is_null() {
            false => {
                let username = CStr::from_ptr(username).to_string_lossy().to_string();
                Some(username)
            }
            true => None,
        };

        let passphrase = match passphrase.is_null() {
            false => {
                let passphrase = CStr::from_ptr(passphrase).to_string_lossy().to_string();
                Some(passphrase)
            }
            true => None,
        };

        let mp = &mut *(ctx);
        mp.get_inner_mut()
            .create_identity(username.as_deref(), passphrase.as_deref())
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_get_identity(
        ctx: *mut MultiPassTraitObject,
        identifier: *mut Identifier,
    ) -> *mut Identity {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        if identifier.is_null() {
            return std::ptr::null_mut();
        }

        let mp = &*(ctx);
        let id = &*(identifier as *mut Identifier);
        match mp.get_inner().get_identity(id.clone()) {
            Ok(identity) => Box::into_raw(Box::new(identity)) as *mut Identity,
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_get_own_identity(
        ctx: *mut MultiPassTraitObject,
    ) -> *mut Identity {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        let mp = &*(ctx);
        match mp.get_inner().get_own_identity() {
            Ok(identity) => Box::into_raw(Box::new(identity)) as *mut Identity,
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_update_identity(
        ctx: *mut MultiPassTraitObject,
        option: *mut IdentityUpdate,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        let mp = &mut *(ctx);
        let option = &*(option as *mut IdentityUpdate);
        mp.get_inner_mut().update_identity(option.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_decrypt_private_key(
        ctx: *mut MultiPassTraitObject,
        passphrase: *const c_char,
    ) -> *const u8 {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let passphrase = match passphrase.is_null() {
            false => {
                let passphrase = CStr::from_ptr(passphrase).to_string_lossy().to_string();
                Some(passphrase)
            }
            true => None,
        };
        let mp = &*(ctx);
        match mp.get_inner().decrypt_private_key(passphrase.as_deref()) {
            Ok(key) => key.as_ptr(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_refresh_cache(ctx: *mut MultiPassTraitObject) {
        if ctx.is_null() {
            return;
        }

        let mp = &mut *(ctx);
        if mp.get_inner_mut().refresh_cache().is_ok() {}
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_send_request(
        ctx: *mut MultiPassTraitObject,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }
        let mp = &mut *(ctx);
        let pk = &*pubkey;
        mp.get_inner_mut().send_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_accept_request(
        ctx: *mut MultiPassTraitObject,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        mp.get_inner_mut().accept_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_deny_request(
        ctx: *mut MultiPassTraitObject,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        mp.get_inner_mut().deny_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_close_request(
        ctx: *mut MultiPassTraitObject,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        mp.get_inner_mut().close_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_incoming_request(
        ctx: *mut MultiPassTraitObject,
    ) -> *const *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &*(ctx);
        match mp.get_inner().list_incoming_request() {
            Ok(list) => {
                let ptr = list.as_ptr() as *const *const _;
                ptr
            }
            Err(_) => std::ptr::null(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_outcoming_request(
        ctx: *mut MultiPassTraitObject,
    ) -> *const *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &*(ctx);
        match mp.get_inner().list_outgoing_request() {
            Ok(list) => {
                let ptr = list.as_ptr() as *const *const _;
                std::mem::forget(list);
                ptr
            }
            Err(_) => std::ptr::null(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_all_request(
        ctx: *mut MultiPassTraitObject,
    ) -> *const *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &*(ctx);
        match mp.get_inner().list_all_request() {
            Ok(list) => {
                let ptr = list.as_ptr() as *const *const _;
                std::mem::forget(list);
                ptr
            }
            Err(_) => std::ptr::null(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_remove_friend(
        ctx: *mut MultiPassTraitObject,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        mp.get_inner_mut().remove_friend(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_block_key(
        ctx: *mut MultiPassTraitObject,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        mp.get_inner_mut().block_key(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_friends(
        ctx: *mut MultiPassTraitObject,
    ) -> *const *const Identity {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        let mp = &*(ctx);
        match mp.get_inner().list_friends() {
            Ok(list) => {
                let ptr = list.as_ptr() as *const *const _;
                std::mem::forget(list);
                ptr
            }
            Err(_) => std::ptr::null(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_has_friend(
        ctx: *mut MultiPassTraitObject,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &*(ctx);
        let pk = &*pubkey;
        mp.get_inner().has_friend(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_free(ctx: *mut MultiPassTraitObject) {
        let mp: Box<MultiPassTraitObject> = Box::from_raw(ctx);
        drop(mp)
    }
}

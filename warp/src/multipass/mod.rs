use crate::sync::{Arc, Mutex, MutexGuard};
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;

pub mod generator;
pub mod identity;

use crate::Extension;
use identity::Identity;

#[cfg(not(target_arch = "wasm32"))]
type Result<T> = std::result::Result<T, crate::error::Error>;

#[cfg(target_arch = "wasm32")]
type Result<T> = std::result::Result<T, wasm_bindgen::JsError>;

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
    object: Arc<Mutex<Box<dyn MultiPass>>>,
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
    fn key_exchange(&self, public_key: PublicKey) -> Result<Vec<u8>>;
}

impl MultiPassTraitObject {
    pub fn new(object: Arc<Mutex<Box<dyn MultiPass>>>) -> Self {
        MultiPassTraitObject { object }
    }

    pub fn get_inner(&self) -> Arc<Mutex<Box<dyn MultiPass>>> {
        self.object.clone()
    }

    pub fn inner_guard(&self) -> MutexGuard<Box<dyn MultiPass>> {
        self.object.lock()
    }

    pub fn inner_ptr(&self) -> *const std::ffi::c_void {
        Arc::into_raw(self.object.clone()) as *const std::ffi::c_void
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl MultiPassTraitObject {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn create_identity(
        &mut self,
        username: Option<String>,
        passphrase: Option<String>,
    ) -> Result<PublicKey> {
        self.inner_guard()
            .create_identity(username.as_deref(), passphrase.as_deref())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn update_identity(&mut self, option: IdentityUpdate) -> Result<()> {
        self.inner_guard().update_identity(option)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn decrypt_private_key(&self, passphrase: Option<String>) -> Result<Vec<u8>> {
        self.inner_guard()
            .decrypt_private_key(passphrase.as_deref())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn refresh_cache(&mut self) -> Result<()> {
        self.inner_guard().refresh_cache()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn send_request(&mut self, pubkey: PublicKey) -> Result<()> {
        self.inner_guard().send_request(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn accept_request(&mut self, pubkey: PublicKey) -> Result<()> {
        self.inner_guard().accept_request(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn deny_request(&mut self, pubkey: PublicKey) -> Result<()> {
        self.inner_guard().deny_request(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn close_request(&mut self, pubkey: PublicKey) -> Result<()> {
        self.inner_guard().close_request(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn remove_friend(&mut self, pubkey: PublicKey) -> Result<()> {
        self.inner_guard().remove_friend(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn block_key(&mut self, pubkey: PublicKey) -> Result<()> {
        self.inner_guard().block_key(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn has_friend(&self, pubkey: PublicKey) -> Result<()> {
        self.inner_guard().has_friend(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn key_exchange(&self, pubkey: PublicKey) -> Result<Vec<u8>> {
        self.inner_guard().key_exchange(pubkey)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl MultiPassTraitObject {
    pub fn get_identity(&self, id: Identifier) -> Result<Identity> {
        self.inner_guard().get_identity(id)
    }

    pub fn get_own_identity(&self) -> Result<Identity> {
        self.inner_guard().get_own_identity()
    }

    pub fn list_incoming_request(&self) -> Result<Vec<FriendRequest>> {
        self.inner_guard().list_incoming_request()
    }

    pub fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>> {
        self.inner_guard().list_outgoing_request()
    }

    pub fn list_friends(&self) -> Result<Vec<Identity>> {
        self.inner_guard().list_friends()
    }

    pub fn list_all_request(&self) -> Result<Vec<FriendRequest>> {
        self.inner_guard().list_all_request()
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        #[wasm_bindgen]
        impl MultiPassTraitObject {

            #[wasm_bindgen]
            pub fn get_identity(&self, id: Identifier) -> Result<JsValue> {
                self.inner_guard().get_identity(id).map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
            }

            #[wasm_bindgen]
            pub fn get_own_identity(&self) -> Result<JsValue> {
                self.inner_guard().get_own_identity().map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
            }

            #[wasm_bindgen]
            pub fn list_incoming_request(&self) -> Result<JsValue> {
                self.inner_guard().list_incoming_request().map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
            }

            #[wasm_bindgen]
            pub fn list_outgoing_request(&self) -> Result<JsValue> {
                self.inner_guard().list_outgoing_request().map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
            }

            #[wasm_bindgen]
            pub fn list_friends(&self) -> Result<JsValue> {
                self.inner_guard().list_friends().map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
            }

            #[wasm_bindgen]
            pub fn list_all_request(&self) -> Result<JsValue> {
                self.inner_guard().list_all_request().map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
            }
        }
    }
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
        mp.inner_guard()
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
        match mp.inner_guard().get_identity(id.clone()) {
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
        match mp.inner_guard().get_own_identity() {
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
        mp.inner_guard().update_identity(option.clone()).is_ok()
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
        match mp.inner_guard().decrypt_private_key(passphrase.as_deref()) {
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
        if mp.inner_guard().refresh_cache().is_ok() {}
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
        mp.inner_guard().send_request(pk.clone()).is_ok()
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
        mp.inner_guard().accept_request(pk.clone()).is_ok()
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
        mp.inner_guard().deny_request(pk.clone()).is_ok()
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
        mp.inner_guard().close_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_incoming_request(
        ctx: *mut MultiPassTraitObject,
    ) -> *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &*(ctx);
        match mp.inner_guard().list_incoming_request() {
            Ok(list) => {
                let ptr = list.as_ptr();
                std::mem::forget(list);
                ptr
            }
            Err(_) => std::ptr::null(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_outcoming_request(
        ctx: *mut MultiPassTraitObject,
    ) -> *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &*(ctx);
        match mp.inner_guard().list_outgoing_request() {
            Ok(list) => {
                let ptr = list.as_ptr();
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
    ) -> *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &*(ctx);
        match mp.inner_guard().list_all_request() {
            Ok(list) => {
                let ptr = list.as_ptr();
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
        mp.inner_guard().remove_friend(pk.clone()).is_ok()
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
        mp.inner_guard().block_key(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_friends(
        ctx: *mut MultiPassTraitObject,
    ) -> *const Identity {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        let mp = &*(ctx);
        match mp.inner_guard().list_friends() {
            Ok(list) => {
                let ptr = list.as_ptr();
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
        mp.inner_guard().has_friend(pk.clone()).is_ok()
    }

    //TODO: Key Exchange

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_free(ctx: *mut MultiPassTraitObject) {
        let mp: Box<MultiPassTraitObject> = Box::from_raw(ctx);
        drop(mp)
    }
}

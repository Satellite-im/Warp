use crate::sync::{Arc, Mutex, MutexGuard};
use warp_derive::FFIFree;
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;

pub mod generator;
pub mod identity;

use crate::Extension;
use identity::Identity;

type Result<T> = std::result::Result<T, crate::error::Error>;
use crate::crypto::PublicKey;
use crate::multipass::identity::{FriendRequest, Identifier, IdentityUpdate};

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
#[derive(FFIFree)]
pub struct MultiPassAdapter {
    object: Arc<Mutex<Box<dyn MultiPass>>>,
}

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
}

impl MultiPassAdapter {
    pub fn new(object: Arc<Mutex<Box<dyn MultiPass>>>) -> Self {
        MultiPassAdapter { object }
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
impl MultiPassAdapter {
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
    pub fn get_identity(&self, id: Identifier) -> Result<Identity> {
        self.inner_guard().get_identity(id)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn get_own_identity(&self) -> Result<Identity> {
        self.inner_guard().get_own_identity()
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
    pub fn id(&self) -> String {
        self.inner_guard().id()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn name(&self) -> String {
        self.inner_guard().name()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn description(&self) -> String {
        self.inner_guard().description()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn module(&self) -> crate::module::Module {
        self.inner_guard().module()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl MultiPassAdapter {
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
        impl MultiPassAdapter {
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
    use crate::crypto::PublicKey;
    use crate::error::Error;
    use crate::ffi::{FFIArray, FFIResult};
    use crate::multipass::{
        identity::{FriendRequest, Identifier, Identity, IdentityUpdate},
        MultiPassAdapter,
    };
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_void};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_create_identity(
        ctx: *mut MultiPassAdapter,
        username: *const c_char,
        passphrase: *const c_char,
    ) -> FFIResult<PublicKey> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
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
        match mp
            .inner_guard()
            .create_identity(username.as_deref(), passphrase.as_deref())
        {
            Ok(pkey) => FFIResult::ok(pkey),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_get_identity(
        ctx: *const MultiPassAdapter,
        identifier: *const Identifier,
    ) -> FFIResult<Identity> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if identifier.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &*(ctx);
        let id = &*(identifier as *mut Identifier);
        match mp.inner_guard().get_identity(id.clone()) {
            Ok(identity) => FFIResult::ok(identity),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_get_own_identity(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult<Identity> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &*(ctx);
        match mp.inner_guard().get_own_identity() {
            Ok(identity) => FFIResult::ok(identity),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_update_identity(
        ctx: *mut MultiPassAdapter,
        option: *const IdentityUpdate,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &mut *(ctx);
        let option = &*option;
        FFIResult::from(mp.inner_guard().update_identity(option.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_decrypt_private_key(
        ctx: *mut MultiPassAdapter,
        passphrase: *const c_char,
    ) -> FFIResult<FFIArray<u8>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Argument is null")));
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
            Ok(key) => FFIResult::ok(FFIArray::new(key)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_refresh_cache(
        ctx: *mut MultiPassAdapter,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &mut *(ctx);
        FFIResult::from(mp.inner_guard().refresh_cache())
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_send_request(
        ctx: *mut MultiPassAdapter,
        pubkey: *const PublicKey,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }
        let mp = &mut *(ctx);
        let pk = &*pubkey;
        FFIResult::from(mp.inner_guard().send_request(pk.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_accept_request(
        ctx: *mut MultiPassAdapter,
        pubkey: *const PublicKey,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        FFIResult::from(mp.inner_guard().accept_request(pk.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_deny_request(
        ctx: *mut MultiPassAdapter,
        pubkey: *const PublicKey,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        FFIResult::from(mp.inner_guard().deny_request(pk.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_close_request(
        ctx: *mut MultiPassAdapter,
        pubkey: *const PublicKey,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        FFIResult::from(mp.inner_guard().close_request(pk.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_incoming_request(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult<FFIArray<FriendRequest>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &*(ctx);
        match mp.inner_guard().list_incoming_request() {
            Ok(list) => FFIResult::ok(FFIArray::new(list)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_outgoing_request(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult<FFIArray<FriendRequest>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &*(ctx);
        match mp.inner_guard().list_outgoing_request() {
            Ok(list) => FFIResult::ok(FFIArray::new(list)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_all_request(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult<FFIArray<FriendRequest>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &*(ctx);
        match mp.inner_guard().list_all_request() {
            Ok(list) => FFIResult::ok(FFIArray::new(list)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_remove_friend(
        ctx: *mut MultiPassAdapter,
        pubkey: *const PublicKey,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        FFIResult::from(mp.inner_guard().remove_friend(pk.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_block_key(
        ctx: *mut MultiPassAdapter,
        pubkey: *const PublicKey,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        FFIResult::from(mp.inner_guard().block_key(pk.clone()))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_friends(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult<FFIArray<Identity>> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &*(ctx);
        match mp.inner_guard().list_friends() {
            Ok(list) => FFIResult::ok(FFIArray::new(list)),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_has_friend(
        ctx: *const MultiPassAdapter,
        pubkey: *const PublicKey,
    ) -> FFIResult<c_void> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Public key cannot be null")));
        }

        let mp = &*(ctx);
        let pk = &*pubkey;

        FFIResult::from(mp.inner_guard().has_friend(pk.clone()))
    }
}

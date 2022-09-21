pub mod generator;
pub mod identity;

use warp_derive::FFIFree;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::error::Error;
use crate::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::{Extension, SingleHandle};
use identity::Identity;

use crate::crypto::DID;
use crate::multipass::identity::{FriendRequest, Identifier, IdentityUpdate};

pub trait MultiPass: Extension + Friends + Sync + Send + SingleHandle {
    /// Create an [`Identity`]
    fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<DID, Error>;

    /// Obtain an [`Identity`] using [`Identifier`]
    fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error>;

    /// Obtain your own [`Identity`]
    fn get_own_identity(&self) -> Result<Identity, Error> {
        self.get_identity(Identifier::own())
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))
    }

    /// Update your own [`Identity`] using [`IdentityUpdate`]
    fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error>;

    /// Decrypt and provide private key for [`Identity`]
    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<DID, Error>;

    /// Clear out cache related to [`Module::Accounts`]
    fn refresh_cache(&mut self) -> Result<(), Error>;
}

pub trait Friends: Sync + Send {
    /// Send friend request to corresponding public key
    fn send_request(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Accept friend request from public key
    fn accept_request(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Deny friend request from public key
    fn deny_request(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Closing or retracting friend request
    fn close_request(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    fn received_friend_request_from(&self, _: &DID) -> Result<bool, Error> {
        Err(Error::Unimplemented)
    }

    /// List the incoming friend request
    fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Err(Error::Unimplemented)
    }

    fn sent_friend_request_to(&self, _: &DID) -> Result<bool, Error> {
        Err(Error::Unimplemented)
    }

    /// List the outgoing friend request
    fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Err(Error::Unimplemented)
    }

    /// List all the friend request that been sent or received
    fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Err(Error::Unimplemented)
    }

    /// Remove friend from contacts
    fn remove_friend(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Block public key, rather it be a friend or not, from being able to send request to account public address
    fn block(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Unblock public key
    fn unblock(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// List block list
    fn block_list(&self) -> Result<Vec<DID>, Error> {
        Err(Error::Unimplemented)
    }

    /// Check to see if public key is blocked
    fn is_blocked(&self, _: &DID) -> Result<bool, Error> {
        Err(Error::Unimplemented)
    }

    /// List all friends public key
    fn list_friends(&self) -> Result<Vec<DID>, Error> {
        Err(Error::Unimplemented)
    }

    /// Check to see if public key is friend of the account
    fn has_friend(&self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(FFIFree)]
pub struct MultiPassAdapter {
    object: Arc<RwLock<Box<dyn MultiPass>>>,
}

impl MultiPassAdapter {
    pub fn new(object: Arc<RwLock<Box<dyn MultiPass>>>) -> Self {
        MultiPassAdapter { object }
    }

    pub fn inner(&self) -> Arc<RwLock<Box<dyn MultiPass>>> {
        self.object.clone()
    }

    pub fn read_guard(&self) -> RwLockReadGuard<Box<dyn MultiPass>> {
        self.object.read()
    }

    pub fn write_guard(&mut self) -> RwLockWriteGuard<Box<dyn MultiPass>> {
        self.object.write()
    }
}

//TODO: Complete when time for multipass implementation
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl MultiPassAdapter {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn create_identity(
        &mut self,
        username: Option<String>,
        passphrase: Option<String>,
    ) -> Result<DID, Error> {
        self.write_guard()
            .create_identity(username.as_deref(), passphrase.as_deref())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error> {
        self.read_guard().get_identity(id)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn get_own_identity(&self) -> Result<Identity, Error> {
        self.read_guard().get_own_identity()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        self.write_guard().update_identity(option)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn decrypt_private_key(&self, passphrase: Option<String>) -> Result<DID, Error> {
        self.read_guard().decrypt_private_key(passphrase.as_deref())
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn refresh_cache(&mut self) -> Result<(), Error> {
        self.write_guard().refresh_cache()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        self.write_guard().send_request(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        self.write_guard().accept_request(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn deny_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        self.write_guard().deny_request(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
        self.write_guard().close_request(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn remove_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
        self.write_guard().remove_friend(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn has_friend(&self, pubkey: &DID) -> Result<(), Error> {
        self.read_guard().has_friend(pubkey)
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn id(&self) -> String {
        self.read_guard().id()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn name(&self) -> String {
        self.read_guard().name()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn description(&self) -> String {
        self.read_guard().description()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn module(&self) -> crate::module::Module {
        self.read_guard().module()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl MultiPassAdapter {
    pub fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.read_guard().list_incoming_request()
    }

    pub fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.read_guard().list_outgoing_request()
    }

    pub fn list_friends(&self) -> Result<Vec<DID>, Error> {
        self.read_guard().list_friends()
    }

    pub fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.read_guard().list_all_request()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl MultiPassAdapter {
    #[wasm_bindgen]
    pub fn list_incoming_request(&self) -> Result<JsValue, Error> {
        self.read_guard()
            .list_incoming_request()
            .map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
    }

    #[wasm_bindgen]
    pub fn list_outgoing_request(&self) -> Result<JsValue, Error> {
        self.read_guard()
            .list_outgoing_request()
            .map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
    }

    #[wasm_bindgen]
    pub fn list_friends(&self) -> Result<JsValue, Error> {
        self.read_guard()
            .list_friends()
            .map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
    }

    #[wasm_bindgen]
    pub fn list_all_request(&self) -> Result<JsValue, Error> {
        self.read_guard()
            .list_all_request()
            .map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::async_on_block;
    use crate::crypto::{FFIResult_FFIVec_DID, DID};
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null};
    use crate::multipass::{
        identity::{
            FFIResult_FFIVec_FriendRequest, FFIResult_FFIVec_Identity, Identifier, Identity,
            IdentityUpdate,
        },
        MultiPassAdapter,
    };
    use std::ffi::CStr;
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_create_identity(
        ctx: *mut MultiPassAdapter,
        username: *const c_char,
        passphrase: *const c_char,
    ) -> FFIResult<DID> {
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

        match async_on_block(async {
            mp.write_guard()
                .create_identity(username.as_deref(), passphrase.as_deref())
        }) {
            Ok(pkey) => FFIResult::ok(pkey),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_get_identity(
        ctx: *const MultiPassAdapter,
        identifier: *const Identifier,
    ) -> FFIResult_FFIVec_Identity {
        if ctx.is_null() {
            return FFIResult_FFIVec_Identity::err(Error::Any(anyhow::anyhow!(
                "Context cannot be null"
            )));
        }

        if identifier.is_null() {
            return FFIResult_FFIVec_Identity::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &*(ctx);
        let id = &*(identifier);
        async_on_block(async { mp.read_guard().get_identity(id.clone()) }).into()
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
        match async_on_block(async { mp.read_guard().get_own_identity() }) {
            Ok(identity) => FFIResult::ok(identity),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_update_identity(
        ctx: *mut MultiPassAdapter,
        option: *const IdentityUpdate,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &mut *(ctx);
        let option = &*option;
        async_on_block(async { mp.write_guard().update_identity(option.clone()) }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_decrypt_private_key(
        ctx: *mut MultiPassAdapter,
        passphrase: *const c_char,
    ) -> FFIResult<DID> {
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
        match async_on_block(async { mp.read_guard().decrypt_private_key(passphrase.as_deref()) }) {
            Ok(key) => FFIResult::ok(key),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_refresh_cache(ctx: *mut MultiPassAdapter) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &mut *(ctx);
        async_on_block(async { mp.write_guard().refresh_cache() }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_send_request(
        ctx: *mut MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }
        let mp = &mut *(ctx);
        let pk = &*pubkey;
        async_on_block(async { mp.write_guard().send_request(pk) }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_accept_request(
        ctx: *mut MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        async_on_block(async { mp.write_guard().accept_request(pk) }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_deny_request(
        ctx: *mut MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        async_on_block(async { mp.write_guard().deny_request(pk) }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_close_request(
        ctx: *mut MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        async_on_block(async { mp.write_guard().close_request(pk) }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_incoming_request(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult_FFIVec_FriendRequest {
        if ctx.is_null() {
            return FFIResult_FFIVec_FriendRequest::err(Error::Any(anyhow::anyhow!(
                "Context cannot be null"
            )));
        }

        let mp = &*(ctx);
        async_on_block(async { mp.read_guard().list_incoming_request() }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_outgoing_request(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult_FFIVec_FriendRequest {
        if ctx.is_null() {
            return FFIResult_FFIVec_FriendRequest::err(Error::Any(anyhow::anyhow!(
                "Context cannot be null"
            )));
        }

        let mp = &*(ctx);
        async_on_block(async { mp.read_guard().list_outgoing_request() }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_all_request(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult_FFIVec_FriendRequest {
        if ctx.is_null() {
            return FFIResult_FFIVec_FriendRequest::err(Error::Any(anyhow::anyhow!(
                "Context cannot be null"
            )));
        }

        let mp = &*(ctx);
        async_on_block(async { mp.read_guard().list_all_request() }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_remove_friend(
        ctx: *mut MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        async_on_block(async { mp.write_guard().remove_friend(pk) }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_block(
        ctx: *mut MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        async_on_block(async { mp.write_guard().block(pk) }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_unblock(
        ctx: *mut MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Argument is null")));
        }

        let mp = &mut *(ctx);
        let pk = &*pubkey;
        async_on_block(async { mp.write_guard().unblock(pk) }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_block_list(
        ctx: *mut MultiPassAdapter,
    ) -> FFIResult_FFIVec_DID {
        if ctx.is_null() {
            return FFIResult_FFIVec_DID::err(Error::Any(anyhow::anyhow!(
                "Context cannot be null"
            )));
        }

        let mp = &mut *ctx;
        async_on_block(async { mp.read_guard().block_list() }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_friends(
        ctx: *const MultiPassAdapter,
    ) -> FFIResult_FFIVec_DID {
        if ctx.is_null() {
            return FFIResult_FFIVec_DID::err(Error::Any(anyhow::anyhow!(
                "Context cannot be null"
            )));
        }

        let mp = &*(ctx);
        async_on_block(async { mp.read_guard().list_friends() }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_has_friend(
        ctx: *const MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult_Null {
        if ctx.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult_Null::err(Error::Any(anyhow::anyhow!("Public key cannot be null")));
        }

        let mp = &*(ctx);
        let pk = &*pubkey;

        async_on_block(async { mp.read_guard().has_friend(pk) }).into()
    }
}

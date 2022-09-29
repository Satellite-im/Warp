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

use self::identity::{IdentityStatus, Relationship};

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
pub trait MultiPass:
    Extension + IdentityInformation + Friends + Sync + Send + SingleHandle
{
    /// Create an [`Identity`]
    async fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<DID, Error>;

    /// Obtain an [`Identity`] using [`Identifier`]
    async fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error>;

    /// Obtain your own [`Identity`]
    async fn get_own_identity(&self) -> Result<Identity, Error> {
        self.get_identity(Identifier::own())
            .await
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))
    }

    /// Update your own [`Identity`] using [`IdentityUpdate`]
    async fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error>;

    /// Decrypt and provide private key for [`Identity`]
    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<DID, Error>;

    /// Clear out cache related to [`Module::Accounts`]
    fn refresh_cache(&mut self) -> Result<(), Error>;
}

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
impl<T: ?Sized> MultiPass for Arc<RwLock<Box<T>>>
where
    T: MultiPass,
{
    async fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<DID, Error> {
        self.write().create_identity(username, passphrase).await
    }

    async fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error> {
        self.read().get_identity(id).await
    }

    async fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        self.write().update_identity(option).await
    }

    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<DID, Error> {
        self.read().decrypt_private_key(passphrase)
    }

    fn refresh_cache(&mut self) -> Result<(), Error> {
        self.write().refresh_cache()
    }
}

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
pub trait Friends: Sync + Send {
    /// Send friend request to corresponding public key
    async fn send_request(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Accept friend request from public key
    async fn accept_request(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Deny friend request from public key
    async fn deny_request(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Closing or retracting friend request
    async fn close_request(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Check to determine if a request been received from the DID
    async fn received_friend_request_from(&self, _: &DID) -> Result<bool, Error> {
        Err(Error::Unimplemented)
    }

    /// List the incoming friend request
    async fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Err(Error::Unimplemented)
    }

    /// Check to determine if a request been sent to the DID
    async fn sent_friend_request_to(&self, _: &DID) -> Result<bool, Error> {
        Err(Error::Unimplemented)
    }

    /// List the outgoing friend request
    async fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Err(Error::Unimplemented)
    }

    /// List all the friend request that been sent or received
    async fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        Err(Error::Unimplemented)
    }

    /// Remove friend from contacts
    async fn remove_friend(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Block public key, rather it be a friend or not, from being able to send request to account public address
    async fn block(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Unblock public key
    async fn unblock(&mut self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// List block list
    async fn block_list(&self) -> Result<Vec<DID>, Error> {
        Err(Error::Unimplemented)
    }

    /// Check to see if public key is blocked
    async fn is_blocked(&self, _: &DID) -> Result<bool, Error> {
        Err(Error::Unimplemented)
    }

    /// List all friends public key
    async fn list_friends(&self) -> Result<Vec<DID>, Error> {
        Err(Error::Unimplemented)
    }

    /// Check to see if public key is friend of the account
    async fn has_friend(&self, _: &DID) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
impl<T: ?Sized> Friends for Arc<RwLock<Box<T>>>
where
    T: Friends,
{
    async fn send_request(&mut self, key: &DID) -> Result<(), Error> {
        self.write().send_request(key).await
    }

    /// Accept friend request from public key
    async fn accept_request(&mut self, key: &DID) -> Result<(), Error> {
        self.write().accept_request(key).await
    }

    /// Deny friend request from public key
    async fn deny_request(&mut self, key: &DID) -> Result<(), Error> {
        self.write().deny_request(key).await
    }

    /// Closing or retracting friend request
    async fn close_request(&mut self, key: &DID) -> Result<(), Error> {
        self.write().close_request(key).await
    }

    /// List the incoming friend request
    async fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.read().list_incoming_request().await
    }

    /// Check to determine if a request been received from the DID
    async fn received_friend_request_from(&self, did: &DID) -> Result<bool, Error> {
        self.read().received_friend_request_from(did).await
    }

    async fn sent_friend_request_to(&self, did: &DID) -> Result<bool, Error> {
        self.read().sent_friend_request_to(did).await
    }

    /// List the outgoing friend request
    async fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.read().list_outgoing_request().await
    }

    /// List all the friend request that been sent or received
    async fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
        self.read().list_all_request().await
    }

    /// Remove friend from contacts
    async fn remove_friend(&mut self, key: &DID) -> Result<(), Error> {
        self.write().remove_friend(key).await
    }

    /// Block public key, rather it be a friend or not, from being able to send request to account public address
    async fn block(&mut self, key: &DID) -> Result<(), Error> {
        self.write().block(key).await
    }

    /// Unblock public key
    async fn unblock(&mut self, key: &DID) -> Result<(), Error> {
        self.write().unblock(key).await
    }

    /// List block list
    async fn block_list(&self) -> Result<Vec<DID>, Error> {
        self.read().block_list().await
    }

    /// Check to see if public key is blocked
    async fn is_blocked(&self, did: &DID) -> Result<bool, Error> {
        self.read().is_blocked(did).await
    }
    /// List all friends public key
    async fn list_friends(&self) -> Result<Vec<DID>, Error> {
        self.read().list_friends().await
    }

    /// Check to see if public key is friend of the account
    async fn has_friend(&self, key: &DID) -> Result<(), Error> {
        self.write().has_friend(key).await
    }
}

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
pub trait IdentityInformation: Send + Sync {
    /// Identity status to determine if they are online or offline
    async fn identity_status(&self, _: &DID) -> Result<IdentityStatus, Error> {
        Err(Error::Unimplemented)
    }
    /// Find the relationship with an existing identity.
    async fn identity_relationship(&self, _: &DID) -> Result<Relationship, Error> {
        Err(Error::Unimplemented)
    }
}

#[allow(clippy::await_holding_lock)]
#[async_trait::async_trait]
impl<T: ?Sized> IdentityInformation for Arc<RwLock<Box<T>>>
where
    T: IdentityInformation,
{
    /// Identity status to determine if they are online or offline
    async fn identity_status(&self, did: &DID) -> Result<IdentityStatus, Error> {
        self.read().identity_status(did).await
    }
    /// Find the relationship with an existing identity.
    async fn identity_relationship(&self, did: &DID) -> Result<Relationship, Error> {
        self.read().identity_relationship(did).await
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

//TODO: Determine if this should be used for wasm
// #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
// impl MultiPassAdapter {
//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn create_identity(
//         &mut self,
//         username: Option<String>,
//         passphrase: Option<String>,
//     ) -> Result<DID, Error> {
//         self.write_guard()
//             .create_identity(username.as_deref(), passphrase.as_deref())
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error> {
//         self.read_guard().get_identity(id)
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn get_own_identity(&self) -> Result<Identity, Error> {
//         self.read_guard().get_own_identity()
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
//         self.write_guard().update_identity(option)
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn decrypt_private_key(&self, passphrase: Option<String>) -> Result<DID, Error> {
//         self.read_guard().decrypt_private_key(passphrase.as_deref())
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn refresh_cache(&mut self) -> Result<(), Error> {
//         self.write_guard().refresh_cache()
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn send_request(&mut self, pubkey: &DID) -> Result<(), Error> {
//         self.write_guard().send_request(pubkey)
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn accept_request(&mut self, pubkey: &DID) -> Result<(), Error> {
//         self.write_guard().accept_request(pubkey)
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn deny_request(&mut self, pubkey: &DID) -> Result<(), Error> {
//         self.write_guard().deny_request(pubkey)
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn close_request(&mut self, pubkey: &DID) -> Result<(), Error> {
//         self.write_guard().close_request(pubkey)
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn remove_friend(&mut self, pubkey: &DID) -> Result<(), Error> {
//         self.write_guard().remove_friend(pubkey)
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn has_friend(&self, pubkey: &DID) -> Result<(), Error> {
//         self.read_guard().has_friend(pubkey)
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn id(&self) -> String {
//         self.read_guard().id()
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn name(&self) -> String {
//         self.read_guard().name()
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn description(&self) -> String {
//         self.read_guard().description()
//     }

//     #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
//     pub fn module(&self) -> crate::module::Module {
//         self.read_guard().module()
//     }
// }

// #[cfg(not(target_arch = "wasm32"))]
// impl MultiPassAdapter {
//     pub fn list_incoming_request(&self) -> Result<Vec<FriendRequest>, Error> {
//         self.read_guard().list_incoming_request()
//     }

//     pub fn list_outgoing_request(&self) -> Result<Vec<FriendRequest>, Error> {
//         self.read_guard().list_outgoing_request()
//     }

//     pub fn list_friends(&self) -> Result<Vec<DID>, Error> {
//         self.read_guard().list_friends()
//     }

//     pub fn list_all_request(&self) -> Result<Vec<FriendRequest>, Error> {
//         self.read_guard().list_all_request()
//     }
// }

// #[cfg(target_arch = "wasm32")]
// #[wasm_bindgen]
// impl MultiPassAdapter {
//     #[wasm_bindgen]
//     pub fn list_incoming_request(&self) -> Result<JsValue, Error> {
//         self.read_guard()
//             .list_incoming_request()
//             .map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
//     }

//     #[wasm_bindgen]
//     pub fn list_outgoing_request(&self) -> Result<JsValue, Error> {
//         self.read_guard()
//             .list_outgoing_request()
//             .map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
//     }

//     #[wasm_bindgen]
//     pub fn list_friends(&self) -> Result<JsValue, Error> {
//         self.read_guard()
//             .list_friends()
//             .map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
//     }

//     #[wasm_bindgen]
//     pub fn list_all_request(&self) -> Result<JsValue, Error> {
//         self.read_guard()
//             .list_all_request()
//             .map(|v| serde_wasm_bindgen::to_value(&v).unwrap())
//     }
// }

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

    use super::identity::{IdentityStatus, Relationship};

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

        match async_on_block(
            mp.write_guard()
                .create_identity(username.as_deref(), passphrase.as_deref()),
        ) {
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
        async_on_block(mp.read_guard().get_identity(id.clone())).into()
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
        match async_on_block(mp.read_guard().get_own_identity()) {
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
        async_on_block(mp.write_guard().update_identity(option.clone())).into()
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
        async_on_block(mp.write_guard().send_request(pk)).into()
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
        async_on_block(mp.write_guard().accept_request(pk)).into()
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
        async_on_block(mp.write_guard().deny_request(pk)).into()
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
        async_on_block(mp.write_guard().close_request(pk)).into()
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
        async_on_block(mp.read_guard().list_incoming_request()).into()
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
        async_on_block(mp.read_guard().list_outgoing_request()).into()
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
        async_on_block(mp.read_guard().list_all_request()).into()
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
        async_on_block(mp.write_guard().remove_friend(pk)).into()
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
        async_on_block(mp.write_guard().block(pk)).into()
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
        async_on_block(mp.write_guard().unblock(pk)).into()
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
        async_on_block(mp.read_guard().block_list()).into()
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
        async_on_block(mp.read_guard().list_friends()).into()
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

        async_on_block(mp.read_guard().has_friend(pk)).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_received_friend_request_from(
        ctx: *const MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult<bool> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Public key cannot be null")));
        }

        let mp = &*(ctx);
        let pk = &*pubkey;

        async_on_block(mp.read_guard().received_friend_request_from(pk)).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_sent_friend_request_to(
        ctx: *const MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult<bool> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Public key cannot be null")));
        }

        let mp = &*(ctx);
        let pk = &*pubkey;

        async_on_block(mp.read_guard().sent_friend_request_to(pk)).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_is_blocked(
        ctx: *const MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult<bool> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Public key cannot be null")));
        }

        let mp = &*(ctx);
        let pk = &*pubkey;

        async_on_block(mp.read_guard().is_blocked(pk)).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_status(
        ctx: *const MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult<IdentityStatus> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Public key cannot be null")));
        }

        let mp = &*(ctx);
        let pk = &*pubkey;

        async_on_block(mp.read_guard().identity_status(pk)).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_relationship(
        ctx: *const MultiPassAdapter,
        pubkey: *const DID,
    ) -> FFIResult<Relationship> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        if pubkey.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Public key cannot be null")));
        }

        let mp = &*(ctx);
        let pk = &*pubkey;

        async_on_block(mp.read_guard().identity_relationship(pk)).into()
    }
}

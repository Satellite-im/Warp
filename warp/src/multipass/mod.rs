pub mod generator;
pub mod identity;

use dyn_clone::DynClone;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use warp_derive::FFIFree;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::error::Error;

use crate::{Extension, SingleHandle};
use identity::Identity;

use crate::crypto::DID;
use crate::multipass::identity::{FriendRequest, Identifier, IdentityUpdate};

use self::identity::{IdentityStatus, Platform, Relationship};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, warp_derive::FFIVec, FFIFree)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum MultiPassEventKind {
    FriendRequestReceived { from: DID },
    FriendRequestSent { to: DID },
    IncomingFriendRequestRejected { did: DID },
    OutgoingFriendRequestRejected { did: DID },
    IncomingFriendRequestClosed { did: DID },
    OutgoingFriendRequestClosed { did: DID },
    FriendAdded { did: DID },
    FriendRemoved { did: DID },
    IdentityOnline { did: DID },
    IdentityOffline { did: DID },
}

#[derive(FFIFree)]
pub struct MultiPassEventStream(pub BoxStream<'static, MultiPassEventKind>);

impl core::ops::Deref for MultiPassEventStream {
    type Target = BoxStream<'static, MultiPassEventKind>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::ops::DerefMut for MultiPassEventStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[async_trait::async_trait]
pub trait MultiPass:
    Extension + IdentityInformation + Friends + FriendsEvent + Sync + Send + SingleHandle + DynClone
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
        self.get_identity(Identifier::own()).await
            .and_then(|list| list.get(0).cloned().ok_or(Error::IdentityDoesntExist))
    }

    /// Update your own [`Identity`] using [`IdentityUpdate`]
    async fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error>;

    /// Decrypt and provide private key for [`Identity`]
    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<DID, Error>;

    /// Clear out cache related to [`Module::Accounts`]
    fn refresh_cache(&mut self) -> Result<(), Error>;
}

dyn_clone::clone_trait_object!(MultiPass);

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

#[async_trait::async_trait]
pub trait FriendsEvent: Sync + Send {
    /// Subscribe to an stream of events
    async fn subscribe(&mut self) -> Result<MultiPassEventStream, Error> {
        Err(Error::Unimplemented)
    }
}


#[async_trait::async_trait]
pub trait IdentityInformation: Send + Sync {
    /// Identity status to determine if they are online or offline
    async fn identity_status(&self, _: &DID) -> Result<IdentityStatus, Error> {
        Err(Error::Unimplemented)
    }

    /// Identity status to determine if they are online or offline
    async fn set_identity_status(&mut self, _: IdentityStatus) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Find the relationship with an existing identity.
    async fn identity_relationship(&self, _: &DID) -> Result<Relationship, Error> {
        Err(Error::Unimplemented)
    }

    /// Returns the identity platform while online.
    async fn identity_platform(&self, _: &DID) -> Result<Platform, Error> {
        Err(Error::Unimplemented)
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Clone, FFIFree)]
pub struct MultiPassAdapter {
    object: Box<dyn MultiPass>,
}

impl MultiPassAdapter {
    pub fn new(object: Box<dyn MultiPass>) -> Self {
        MultiPassAdapter { object }
    }

    pub fn object(&self) -> Box<dyn MultiPass> {
        self.object.clone()
    }
}

impl core::ops::Deref for MultiPassAdapter {
    type Target = Box<dyn MultiPass>;
    fn deref(&self) -> &Self::Target {
        &self.object
    }
}

impl core::ops::DerefMut for MultiPassAdapter {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.object
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use futures::StreamExt;

    use crate::async_on_block;
    use crate::crypto::{FFIResult_FFIVec_DID, DID};
    use crate::error::Error;
    use crate::ffi::{FFIResult, FFIResult_Null, FFIResult_String};
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
    use super::MultiPassEventStream;

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
            mp
                .create_identity(username.as_deref(), passphrase.as_deref()).await
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
        async_on_block(async { mp.get_identity(id.clone()).await }).into()
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
        match async_on_block(async { mp.get_own_identity().await }) {
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
        async_on_block(async { mp.update_identity(option.clone()).await }).into()
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
        match async_on_block(async { mp.decrypt_private_key(passphrase.as_deref()) }) {
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
        async_on_block(async { mp.refresh_cache() }).into()
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
        async_on_block(async { mp.send_request(pk).await }).into()
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
        async_on_block(async { mp.accept_request(pk).await }).into()
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
        async_on_block(async { mp.deny_request(pk).await }).into()
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
        async_on_block(async { mp.close_request(pk).await }).into()
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
        async_on_block(async { mp.list_incoming_request().await }).into()
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
        async_on_block(async { mp.list_outgoing_request().await }).into()
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
        async_on_block(async { mp.list_all_request().await }).into()
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
        async_on_block(async { mp.remove_friend(pk).await }).into()
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
        async_on_block(async { mp.block(pk).await }).into()
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
        async_on_block(async { mp.unblock(pk).await }).into()
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
        async_on_block(async { mp.block_list().await }).into()
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
        async_on_block(async { mp.list_friends().await }).into()
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

        async_on_block(async { mp.has_friend(pk).await }).into()
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

        async_on_block(async { mp.received_friend_request_from(pk).await }).into()
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

        async_on_block(async { mp.sent_friend_request_to(pk).await }).into()
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

        async_on_block(async { mp.is_blocked(pk).await }).into()
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

        async_on_block(async { mp.identity_status(pk).await }).into()
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

        async_on_block(async { mp.identity_relationship(pk).await }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_subscribe(
        ctx: *mut MultiPassAdapter,
    ) -> FFIResult<MultiPassEventStream> {
        if ctx.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let mp = &mut *(ctx);

        async_on_block(async { mp.subscribe().await }).into()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_stream_next(
        ctx: *mut MultiPassEventStream,
    ) -> FFIResult_String {
        if ctx.is_null() {
            return FFIResult_String::err(Error::Any(anyhow::anyhow!("Context cannot be null")));
        }

        let stream = &mut *(ctx);

        match async_on_block(stream.next()) {
            Some(event) => serde_json::to_string(&event).map_err(Error::from).into(),
            None => FFIResult_String::err(Error::Any(anyhow::anyhow!(
                "Error obtaining data from stream"
            ))),
        }
    }
}

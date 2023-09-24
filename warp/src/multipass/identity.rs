use std::fmt::Display;

use crate::crypto::DID;
use chrono::{DateTime, Utc};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use warp_derive::FFIFree;

pub const SHORT_ID_SIZE: usize = 8;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Display, FFIFree)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
pub enum IdentityStatus {
    #[display(fmt = "online")]
    Online,
    #[display(fmt = "away")]
    Away,
    #[display(fmt = "busy")]
    Busy,
    #[display(fmt = "offline")]
    Offline,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq, Display, FFIFree)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
pub enum Platform {
    #[display(fmt = "desktop")]
    Desktop,
    #[display(fmt = "mobile")]
    Mobile,
    #[display(fmt = "web")]
    Web,
    #[display(fmt = "unknown")]
    #[default]
    Unknown,
}

/// Profile containing the newly created `Identity` and a passphrase, if applicable.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct IdentityProfile {
    identity: Identity,
    passphrase: Option<zeroize::Zeroizing<String>>,
}

impl Drop for IdentityProfile {
    fn drop(&mut self) {
        if let Some(passphrase) = self.passphrase.as_mut() {
            zeroize::Zeroize::zeroize(passphrase);
        }
    }
}

impl IdentityProfile {
    pub fn new(identity: Identity, passphrase: Option<String>) -> Self {
        Self {
            identity,
            passphrase: passphrase.map(zeroize::Zeroizing::new),
        }
    }

    /// Reference to `Identity`
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Supplied passphrase, if applicable.
    pub fn passphrase(&self) -> Option<&str> {
        self.passphrase.as_ref().map(|phrase| phrase.as_str())
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, FFIFree)]
pub struct Relationship {
    friends: bool,
    received_friend_request: bool,
    sent_friend_request: bool,
    blocked: bool,
    blocked_by: bool,
}

impl Relationship {
    pub fn set_friends(&mut self, val: bool) {
        self.friends = val;
    }

    pub fn set_received_friend_request(&mut self, val: bool) {
        self.received_friend_request = val;
    }

    pub fn set_sent_friend_request(&mut self, val: bool) {
        self.sent_friend_request = val;
    }

    pub fn set_blocked(&mut self, val: bool) {
        self.blocked = val;
    }

    pub fn set_blocked_by(&mut self, val: bool) {
        self.blocked_by = val;
    }
}

impl Relationship {
    pub fn friends(&self) -> bool {
        self.friends
    }

    pub fn received_friend_request(&self) -> bool {
        self.received_friend_request
    }

    pub fn sent_friend_request(&self) -> bool {
        self.sent_friend_request
    }

    pub fn blocked(&self) -> bool {
        self.blocked
    }

    pub fn blocked_by(&self) -> bool {
        self.blocked_by
    }
}

#[derive(
    Default, Hash, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Serialize, Deserialize, Ord,
)]
pub struct ShortId([u8; SHORT_ID_SIZE]);

impl core::ops::Deref for ShortId {
    type Target = [u8; SHORT_ID_SIZE];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<[u8; SHORT_ID_SIZE]> for ShortId {
    fn from(id: [u8; SHORT_ID_SIZE]) -> Self {
        ShortId(id)
    }
}

impl Display for ShortId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

#[derive(
    Default, Hash, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, warp_derive::FFIVec, FFIFree,
)]
pub struct Identity {
    /// Username of the identity
    username: String,

    /// Short id derived from the DID to be used along side `Identity::username` (eg `Username#0000`)
    short_id: ShortId,

    /// Public key for the identity
    did_key: DID,

    /// Timestamp when the identity was created
    created: DateTime<Utc>,

    /// Timestamp when the identity was last modified or updated
    modified: DateTime<Utc>,

    /// Status message
    status_message: Option<String>,
}

impl Identity {
    pub fn set_username(&mut self, user: &str) {
        self.username = user.to_string()
    }

    pub fn set_short_id<I: Into<ShortId>>(&mut self, id: I) {
        self.short_id = id.into()
    }

    pub fn set_did_key(&mut self, pubkey: DID) {
        self.did_key = pubkey
    }

    pub fn set_status_message(&mut self, message: Option<String>) {
        self.status_message = message
    }

    pub fn set_created(&mut self, time: DateTime<Utc>) {
        self.created = time;
    }

    pub fn set_modified(&mut self, time: DateTime<Utc>) {
        self.modified = time;
    }
}

impl Identity {
    pub fn username(&self) -> String {
        self.username.clone()
    }

    pub fn short_id(&self) -> ShortId {
        self.short_id
    }

    pub fn did_key(&self) -> DID {
        self.did_key.clone()
    }

    pub fn status_message(&self) -> Option<String> {
        self.status_message.clone()
    }

    pub fn created(&self) -> DateTime<Utc> {
        self.created
    }

    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }
}

#[derive(Debug, Clone, FFIFree)]
#[allow(clippy::large_enum_variant)]
pub enum Identifier {
    DID(DID),
    DIDList(Vec<DID>),
    Username(String),
    Own,
}

impl Identifier {
    pub fn user_name(name: &str) -> Self {
        Self::Username(name.to_string())
    }

    pub fn did_key(key: DID) -> Self {
        Self::DID(key)
    }

    pub fn did_keys(keys: Vec<DID>) -> Self {
        Self::DIDList(keys)
    }

    pub fn own() -> Self {
        Self::Own
    }
}

impl From<DID> for Identifier {
    fn from(did_key: DID) -> Self {
        Self::DID(did_key)
    }
}

impl From<String> for Identifier {
    fn from(username: String) -> Self {
        Self::Username(username)
    }
}

impl From<&str> for Identifier {
    fn from(username: &str) -> Self {
        Self::Username(username.to_string())
    }
}

impl From<Vec<DID>> for Identifier {
    fn from(list: Vec<DID>) -> Self {
        Self::DIDList(list)
    }
}

impl From<&[DID]> for Identifier {
    fn from(list: &[DID]) -> Self {
        Self::DIDList(list.to_vec())
    }
}

#[derive(Debug, Clone, FFIFree)]
pub enum IdentityUpdate {
    Username(String),
    Picture(String),
    PicturePath(std::path::PathBuf),
    ClearPicture,
    Banner(String),
    BannerPath(std::path::PathBuf),
    ClearBanner,
    StatusMessage(Option<String>),
    ClearStatusMessage,
}

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

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_set_graphics_picture(
        name: *const c_char,
    ) -> *mut IdentityUpdate {
        if name.is_null() {
            return std::ptr::null_mut();
        }

        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        Box::into_raw(Box::new(IdentityUpdate::Picture(name)))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_set_graphics_banner(
        name: *const c_char,
    ) -> *mut IdentityUpdate {
        if name.is_null() {
            return std::ptr::null_mut();
        }

        let name = CStr::from_ptr(name).to_string_lossy().to_string();

        Box::into_raw(Box::new(IdentityUpdate::Banner(name)))
    }

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

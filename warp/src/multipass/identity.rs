#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::crypto::DID;
use derive_more::Display;
use serde::{Deserialize, Serialize};
use warp_derive::FFIFree;

pub const SHORT_ID_SIZE: usize = 8;

#[derive(
    Default, Hash, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, warp_derive::FFIVec, FFIFree,
)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Role {
    /// Name of the role
    name: String,

    /// TBD
    level: u8,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Role {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn level(&self) -> u8 {
        self.level
    }
}

#[derive(
    Default, Hash, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, warp_derive::FFIVec, FFIFree,
)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Badge {
    /// TBD
    name: String,

    /// TBD
    icon: String,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Badge {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn icon(&self) -> String {
        self.icon.clone()
    }
}

#[derive(Default, Hash, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Graphics {
    /// Hash to profile picture
    profile_picture: String,

    /// Hash to profile banner
    profile_banner: String,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Graphics {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_profile_picture(&mut self, picture: &str) {
        self.profile_picture = picture.to_string();
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_profile_banner(&mut self, banner: &str) {
        self.profile_banner = banner.to_string();
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Graphics {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn profile_picture(&self) -> String {
        self.profile_picture.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn profile_banner(&self) -> String {
        self.profile_banner.clone()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Display, FFIFree)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
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
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
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

#[derive(Default, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
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
    Default, Hash, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, warp_derive::FFIVec, FFIFree,
)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Identity {
    /// Username of the identity
    username: String,

    /// Short id derived from the DID to be used along side `Identity::username` (eg `Username#0000`)
    short_id: [u8; SHORT_ID_SIZE],

    /// Public key for the identity
    did_key: DID,

    /// TBD
    graphics: Graphics,

    /// Status message
    status_message: Option<String>,

    /// List of roles
    roles: Vec<Role>,

    /// List of available badges
    available_badges: Vec<Badge>,

    /// Active badge for identity
    active_badge: Badge,

    /// TBD
    // linked_accounts: HashMap<String, String>,

    /// Signature of the identity
    #[serde(skip_serializing_if = "Option::is_none")]
    signature: Option<String>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Identity {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_username(&mut self, user: &str) {
        self.username = user.to_string()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_short_id(&mut self, id: [u8; SHORT_ID_SIZE]) {
        self.short_id = id
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_did_key(&mut self, pubkey: DID) {
        self.did_key = pubkey
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_graphics(&mut self, graphics: Graphics) {
        self.graphics = graphics
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_status_message(&mut self, message: Option<String>) {
        self.status_message = message
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(setter))]
    pub fn set_signature(&mut self, signature: Option<String>) {
        self.signature = signature;
    }

    // pub fn set_roles(&mut self) {}
    // pub fn set_available_badges(&mut self) {}
    // pub fn set_active_badge(&mut self) {}
    // pub fn set_linked_accounts(&mut self) {}
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Identity {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn username(&self) -> String {
        self.username.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn short_id(&self) -> String {
        String::from_utf8_lossy(&self.short_id).to_string()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn did_key(&self) -> DID {
        self.did_key.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn graphics(&self) -> Graphics {
        self.graphics.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn status_message(&self) -> Option<String> {
        self.status_message.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn active_badge(&self) -> Badge {
        self.active_badge.clone()
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn signature(&self) -> Option<String> {
        self.signature.clone()
    }

    // #[cfg_attr(target_arch = "wasm32", wasm_bindgen(skip))]
    // pub fn linked_accounts(&self) -> HashMap<String, String> {
    //     self.linked_accounts.clone()
    // }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl Identity {
    #[wasm_bindgen]
    pub fn roles(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.roles).unwrap()
    }

    #[wasm_bindgen]
    pub fn available_badges(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.available_badges).unwrap()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Identity {
    pub fn roles(&self) -> Vec<Role> {
        self.roles.clone()
    }

    pub fn available_badges(&self) -> Vec<Badge> {
        self.available_badges.clone()
    }
}

#[derive(Default, Debug, Clone, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Identifier {
    /// Select identity based on DID Key
    did_key: Option<DID>,
    /// Select identity based on Username (eg `Username#0000`)
    user_name: Option<String>,
    /// Select own identity.
    own: bool,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Identifier {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn user_name(name: &str) -> Self {
        Identifier {
            user_name: Some(name.to_string()),
            ..Default::default()
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn did_key(key: DID) -> Self {
        Identifier {
            did_key: Some(key),
            ..Default::default()
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn own() -> Self {
        Identifier {
            own: true,
            ..Default::default()
        }
    }
}

impl Identifier {
    pub fn get_inner(&self) -> (Option<DID>, Option<String>, bool) {
        (self.did_key.clone(), self.user_name.clone(), self.own)
    }
}

impl From<DID> for Identifier {
    fn from(did_key: DID) -> Self {
        Identifier {
            did_key: Some(did_key),
            ..Default::default()
        }
    }
}

impl<S: AsRef<str>> From<S> for Identifier {
    fn from(username: S) -> Self {
        Identifier {
            user_name: Some(username.as_ref().to_string()),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, FFIFree)]
pub enum IdentityUpdate {
    Username(String),
    Picture(String),
    PicturePath(std::path::PathBuf),
    Banner(String),
    BannerPath(std::path::PathBuf),
    StatusMessage(Option<String>),
}

impl IdentityUpdate {
    /// Set new username
    pub fn set_username(username: String) -> IdentityUpdate {
        IdentityUpdate::Username(username)
    }

    /// Set profile picture
    pub fn set_graphics_picture(graphics: String) -> IdentityUpdate {
        IdentityUpdate::Picture(graphics)
    }

    /// Set profile banner
    pub fn set_graphics_banner(graphics: String) -> IdentityUpdate {
        IdentityUpdate::Banner(graphics)
    }

    /// Set status message
    pub fn set_status_message(status_message: Option<String>) -> IdentityUpdate {
        IdentityUpdate::StatusMessage(status_message)
    }
}

impl IdentityUpdate {
    /// Set profile picture from a file
    pub fn set_graphics_picture_path(path: std::path::PathBuf) -> IdentityUpdate {
        IdentityUpdate::PicturePath(path)
    }

    /// Set profile banner from a file
    pub fn set_graphics_banner_path(path: std::path::PathBuf) -> IdentityUpdate {
        IdentityUpdate::BannerPath(path)
    }
}

impl IdentityUpdate {
    pub fn username(&self) -> Option<String> {
        match self.clone() {
            IdentityUpdate::Username(username) => Some(username),
            _ => None,
        }
    }

    pub fn graphics_picture(&self) -> Option<String> {
        match self.clone() {
            IdentityUpdate::Picture(data) => Some(data),
            _ => None,
        }
    }

    pub fn graphics_banner(&self) -> Option<String> {
        match self.clone() {
            IdentityUpdate::Banner(data) => Some(data),
            _ => None,
        }
    }

    pub fn graphics_picture_path(&self) -> Option<std::path::PathBuf> {
        match self.clone() {
            IdentityUpdate::PicturePath(path) => Some(path),
            _ => None,
        }
    }

    pub fn graphics_banner_path(&self) -> Option<std::path::PathBuf> {
        match self.clone() {
            IdentityUpdate::BannerPath(path) => Some(path),
            _ => None,
        }
    }

    pub fn status_message(&self) -> Option<Option<String>> {
        match self.clone() {
            IdentityUpdate::StatusMessage(status) => Some(status),
            _ => None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::crypto::DID;
    use crate::multipass::identity::{
        Badge, FFIVec_Badge, FFIVec_Role, Graphics, Identifier, Identity, IdentityUpdate, Role,
    };
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_void};

    use super::Relationship;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_role_name(role: *const Role) -> *mut c_char {
        if role.is_null() {
            return std::ptr::null_mut();
        }

        let role = &*role;

        match CString::new(role.name()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_role_level(role: *const Role) -> u8 {
        if role.is_null() {
            return 0;
        }

        let role = &*role;

        role.level()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_badge_name(badge: *const Badge) -> *mut c_char {
        if badge.is_null() {
            return std::ptr::null_mut();
        }

        let badge = &*badge;

        match CString::new(badge.name()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_badge_icon(badge: *const Badge) -> *mut c_char {
        if badge.is_null() {
            return std::ptr::null_mut();
        }

        let badge = &*badge;

        match CString::new(badge.icon()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_graphics_profile_picture(
        graphics: *const Graphics,
    ) -> *mut c_char {
        if graphics.is_null() {
            return std::ptr::null_mut();
        }

        let graphics = &*graphics;

        match CString::new(graphics.profile_picture()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_graphics_profile_banner(
        graphics: *const Graphics,
    ) -> *mut c_char {
        if graphics.is_null() {
            return std::ptr::null_mut();
        }

        let graphics = &*graphics;

        match CString::new(graphics.profile_banner()) {
            Ok(c) => c.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }

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
        match CString::new(identity.short_id()) {
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
    pub unsafe extern "C" fn multipass_identity_graphics(
        identity: *const Identity,
    ) -> *mut Graphics {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        Box::into_raw(Box::new(identity.graphics())) as *mut Graphics
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
    pub unsafe extern "C" fn multipass_identity_roles(identity: *mut Identity) -> *mut FFIVec_Role {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        let contents = identity.roles();
        Box::into_raw(Box::new(contents.into())) as *mut _
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_available_badge(
        identity: *mut Identity,
    ) -> *mut FFIVec_Badge {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        let contents = identity.available_badges();
        Box::into_raw(Box::new(contents.into())) as *mut _
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_active_badge(
        identity: *const Identity,
    ) -> *mut Badge {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;
        Box::into_raw(Box::new(identity.active_badge())) as *mut Badge
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_linked_accounts(
        identity: *const Identity,
    ) -> *mut c_void {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let _identity = &*identity;
        //TODO
        std::ptr::null_mut()
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

        Box::into_raw(Box::new(Identifier::user_name(&name))) as *mut Identifier
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identifier_did_key(key: *const DID) -> *mut Identifier {
        if key.is_null() {
            return std::ptr::null_mut();
        }

        let key = &*key;

        Box::into_raw(Box::new(Identifier::did_key(key.clone()))) as *mut Identifier
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identifier_own() -> *mut Identifier {
        Box::into_raw(Box::new(Identifier::own())) as *mut Identifier
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

        Box::into_raw(Box::new(IdentityUpdate::set_username(name))) as *mut IdentityUpdate
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

        Box::into_raw(Box::new(IdentityUpdate::set_graphics_picture(name))) as *mut IdentityUpdate
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

        Box::into_raw(Box::new(IdentityUpdate::set_graphics_banner(name))) as *mut IdentityUpdate
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_set_status_message(
        name: *const c_char,
    ) -> *mut IdentityUpdate {
        let update = if !name.is_null() {
            let name = CStr::from_ptr(name).to_string_lossy().to_string();
            IdentityUpdate::set_status_message(Some(name))
        } else {
            IdentityUpdate::set_status_message(None)
        };

        Box::into_raw(Box::new(update)) as *mut IdentityUpdate
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

        match update.username() {
            Some(data) => match CString::new(data) {
                Ok(data) => data.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            None => std::ptr::null_mut(),
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

        match update.graphics_picture() {
            Some(data) => match CString::new(data) {
                Ok(data) => data.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            None => std::ptr::null_mut(),
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

        match update.graphics_banner() {
            Some(data) => match CString::new(data) {
                Ok(data) => data.into_raw(),
                Err(_) => std::ptr::null_mut(),
            },
            None => std::ptr::null_mut(),
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

        if let Some(Some(inner)) = update.status_message() {
            if let Ok(data) = CString::new(inner) {
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

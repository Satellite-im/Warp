use wasm_bindgen::prelude::*;

use derive_more::Display;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use warp_derive::FFIArray;

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, FFIArray)]
#[wasm_bindgen]
pub struct Role {
    /// Name of the role
    name: String,

    /// TBD
    level: u8,
}

#[wasm_bindgen]
impl Role {
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn level(&self) -> u8 {
        self.level
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, FFIArray)]
#[wasm_bindgen]
pub struct Badge {
    /// TBD
    name: String,

    /// TBD
    icon: String,
}

#[wasm_bindgen]
impl Badge {
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn icon(&self) -> String {
        self.icon.clone()
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[wasm_bindgen]
pub struct Graphics {
    /// Hash to profile picture
    profile_picture: String,

    /// Hash to profile banner
    profile_banner: String,
}

#[wasm_bindgen]
impl Graphics {
    #[wasm_bindgen(setter)]
    pub fn set_profile_picture(&mut self, picture: &str) {
        self.profile_picture = picture.to_string();
    }

    #[wasm_bindgen(setter)]
    pub fn set_profile_banner(&mut self, banner: &str) {
        self.profile_banner = banner.to_string();
    }
}

#[wasm_bindgen]
impl Graphics {
    #[wasm_bindgen(getter)]
    pub fn profile_picture(&self) -> String {
        self.profile_picture.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn profile_banner(&self) -> String {
        self.profile_banner.clone()
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, FFIArray)]
#[wasm_bindgen]
pub struct Identity {
    /// Username of the identity
    username: String,

    /// Short 4-digit numeric id to be used along side `Identity::username` (eg `Username#0000`)
    short_id: u16,

    /// Public key for the identity
    public_key: PublicKey,

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
    linked_accounts: HashMap<String, String>,
}

#[wasm_bindgen]
impl Identity {
    #[wasm_bindgen(setter)]
    pub fn set_username(&mut self, user: &str) {
        self.username = user.to_string()
    }

    #[wasm_bindgen(setter)]
    pub fn set_short_id(&mut self, id: u16) {
        self.short_id = id
    }

    #[wasm_bindgen(setter)]
    pub fn set_public_key(&mut self, pubkey: PublicKey) {
        self.public_key = pubkey
    }

    #[wasm_bindgen(setter)]
    pub fn set_graphics(&mut self, graphics: Graphics) {
        self.graphics = graphics
    }

    #[wasm_bindgen(setter)]
    pub fn set_status_message(&mut self, message: Option<String>) {
        self.status_message = message
    }
    // pub fn set_roles(&mut self) {}
    // pub fn set_available_badges(&mut self) {}
    // pub fn set_active_badge(&mut self) {}
    // pub fn set_linked_accounts(&mut self) {}
}

#[wasm_bindgen]
impl Identity {
    #[wasm_bindgen(getter)]
    pub fn username(&self) -> String {
        self.username.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn short_id(&self) -> u16 {
        self.short_id
    }

    #[wasm_bindgen(getter)]
    pub fn public_key(&self) -> PublicKey {
        self.public_key.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn graphics(&self) -> Graphics {
        self.graphics.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn status_message(&self) -> Option<String> {
        self.status_message.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn active_badge(&self) -> Badge {
        self.active_badge.clone()
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

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, FFIArray)]
#[wasm_bindgen]
pub struct FriendRequest {
    /// The account where the request came from
    from: PublicKey,

    /// The account where the request was sent to
    to: PublicKey,

    /// Status of the request
    status: FriendRequestStatus,
}

#[wasm_bindgen]
impl FriendRequest {
    #[wasm_bindgen(setter)]
    pub fn set_from(&mut self, key: PublicKey) {
        self.from = key
    }

    #[wasm_bindgen(setter)]
    pub fn set_to(&mut self, key: PublicKey) {
        self.to = key
    }

    #[wasm_bindgen(setter)]
    pub fn set_status(&mut self, status: FriendRequestStatus) {
        self.status = status
    }
}

#[wasm_bindgen]
impl FriendRequest {
    #[wasm_bindgen(getter)]
    pub fn from(&self) -> PublicKey {
        self.from.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn to(&self) -> PublicKey {
        self.to.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn status(&self) -> FriendRequestStatus {
        self.status
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Display)]
#[repr(C)]
#[wasm_bindgen]
pub enum FriendRequestStatus {
    #[display(fmt = "uninitialized")]
    Uninitialized,
    #[display(fmt = "pending")]
    Pending,
    #[display(fmt = "accepted")]
    Accepted,
    #[display(fmt = "denied")]
    Denied,
    #[display(fmt = "friend removed")]
    FriendRemoved,
    #[display(fmt = "request removed")]
    RequestRemoved,
}

impl Default for FriendRequestStatus {
    fn default() -> Self {
        Self::Uninitialized
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[wasm_bindgen]
pub struct PublicKey(Vec<u8>);

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[wasm_bindgen]
impl PublicKey {
    #[wasm_bindgen]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    #[wasm_bindgen]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    #[wasm_bindgen]
    pub fn into_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }
}

#[derive(Default, Debug, Clone)]
#[wasm_bindgen]
pub struct Identifier {
    /// Select identity based on public key
    public_key: Option<PublicKey>,
    /// Select identity based on Username (eg `Username#0000`)
    user_name: Option<String>,
    /// Select own identity.
    own: bool,
}

#[wasm_bindgen]
impl Identifier {
    #[wasm_bindgen]
    pub fn user_name(name: &str) -> Self {
        Identifier {
            user_name: Some(name.to_string()),
            ..Default::default()
        }
    }

    #[wasm_bindgen]
    pub fn public_key(key: PublicKey) -> Self {
        Identifier {
            public_key: Some(key),
            ..Default::default()
        }
    }

    #[wasm_bindgen]
    pub fn own() -> Self {
        Identifier {
            own: true,
            ..Default::default()
        }
    }
}

impl Identifier {
    pub fn get_inner(&self) -> (Option<PublicKey>, Option<String>, bool) {
        (self.public_key.clone(), self.user_name.clone(), self.own)
    }
}

impl From<PublicKey> for Identifier {
    fn from(pubkey: PublicKey) -> Self {
        Identifier {
            public_key: Some(pubkey),
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

#[derive(Debug, Clone, Default)]
#[wasm_bindgen]
pub struct IdentityUpdate {
    /// Setting Username
    username: Option<String>,

    /// Path of picture
    graphics_picture: Option<String>,

    /// Path of banner
    graphics_banner: Option<String>,

    /// Setting Status Message
    status_message: Option<Option<String>>,
}

#[wasm_bindgen]
impl IdentityUpdate {
    #[wasm_bindgen]
    pub fn set_username(username: String) -> IdentityUpdate {
        IdentityUpdate {
            username: Some(username),
            ..Default::default()
        }
    }

    #[wasm_bindgen]
    pub fn set_graphics_picture(graphics: String) -> IdentityUpdate {
        IdentityUpdate {
            graphics_picture: Some(graphics),
            ..Default::default()
        }
    }

    #[wasm_bindgen]
    pub fn set_graphics_banner(graphics: String) -> IdentityUpdate {
        IdentityUpdate {
            graphics_banner: Some(graphics),
            ..Default::default()
        }
    }

    #[wasm_bindgen]
    pub fn set_status_message(status_message: Option<String>) -> IdentityUpdate {
        IdentityUpdate {
            status_message: Some(status_message),
            ..Default::default()
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl IdentityUpdate {
    pub fn username(&self) -> Option<String> {
        self.username.clone()
    }

    pub fn graphics_picture(&self) -> Option<String> {
        self.graphics_picture.clone()
    }

    pub fn graphics_banner(&self) -> Option<String> {
        self.graphics_banner.clone()
    }

    pub fn status_message(&self) -> Option<Option<String>> {
        self.status_message.clone()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl IdentityUpdate {
    #[wasm_bindgen(getter)]
    pub fn username(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.username).unwrap()
    }

    #[wasm_bindgen(getter)]
    pub fn graphics_picture(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.graphics_picture).unwrap()
    }

    #[wasm_bindgen(getter)]
    pub fn graphics_banner(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.graphics_banner).unwrap()
    }

    #[wasm_bindgen(getter)]
    pub fn status_message(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.status_message).unwrap()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::ffi::FFIArray;
    use crate::multipass::identity::{
        Badge, FriendRequest, FriendRequestStatus, Graphics, Identifier, Identity, IdentityUpdate,
        PublicKey, Role,
    };
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_void};

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
    pub unsafe extern "C" fn multipass_identity_short_id(identity: *const Identity) -> u16 {
        if identity.is_null() {
            return 0;
        }

        let identity = &*identity;

        identity.short_id()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_public_key(
        identity: *const Identity,
    ) -> *mut PublicKey {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        Box::into_raw(Box::new(identity.public_key())) as *mut PublicKey
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
    pub unsafe extern "C" fn multipass_identity_roles(
        identity: *mut Identity,
    ) -> *mut FFIArray<Role> {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        let contents = identity.roles();
        Box::into_raw(Box::new(FFIArray::new(contents))) as *mut _
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_available_badge(
        identity: *mut Identity,
    ) -> *mut FFIArray<Badge> {
        if identity.is_null() {
            return std::ptr::null_mut();
        }

        let identity = &*identity;

        let contents = identity.available_badges();
        Box::into_raw(Box::new(FFIArray::new(contents))) as *mut _
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
    pub unsafe extern "C" fn multipass_friend_request_from(
        request: *const FriendRequest,
    ) -> *mut PublicKey {
        if request.is_null() {
            return std::ptr::null_mut();
        }

        let request = &*request;
        Box::into_raw(Box::new(request.from())) as *mut PublicKey
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_friend_request_to(
        request: *const FriendRequest,
    ) -> *mut PublicKey {
        if request.is_null() {
            return std::ptr::null_mut();
        }

        let request = &*request;
        Box::into_raw(Box::new(request.to())) as *mut PublicKey
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_friend_request_status(
        request: *const FriendRequest,
    ) -> FriendRequestStatus {
        if request.is_null() {
            return FriendRequestStatus::Uninitialized;
        }

        let request = &*request;
        request.status()
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
    pub unsafe extern "C" fn multipass_identifier_public_key(
        key: *const PublicKey,
    ) -> *mut Identifier {
        if key.is_null() {
            return std::ptr::null_mut();
        }

        let key = &*key;

        Box::into_raw(Box::new(Identifier::public_key(key.clone()))) as *mut Identifier
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identifier_own() -> *const Identifier {
        Box::into_raw(Box::new(Identifier::own())) as *const Identifier
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
        let update = if name.is_null() {
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
    pub unsafe extern "C" fn multipass_public_key_free(ctx: *mut PublicKey) {
        if ctx.is_null() {
            return;
        }
        drop(Box::from_raw(ctx))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identifier_free(ctx: *mut Identifier) {
        if ctx.is_null() {
            return;
        }
        drop(Box::from_raw(ctx))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_update_free(ctx: *mut IdentityUpdate) {
        if ctx.is_null() {
            return;
        }
        drop(Box::from_raw(ctx))
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_identity_free(ctx: *mut Identity) {
        if ctx.is_null() {
            return;
        }
        drop(Box::from_raw(ctx))
    }
}

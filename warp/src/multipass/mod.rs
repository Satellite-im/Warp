pub mod generator;
pub mod identity;

use crate::Extension;
use identity::Identity;

type Result<T> = std::result::Result<T, crate::error::Error>;

use crate::multipass::identity::{FriendRequest, Identifier, IdentityUpdate, PublicKey};

pub trait MultiPass: Extension + Friends + Sync + Send {
    fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<PublicKey>;

    fn get_identity(&self, id: Identifier) -> Result<Identity>;

    fn get_own_identity(&self) -> Result<Identity> {
        self.get_identity(Identifier::Own)
    }

    fn update_identity(&mut self, option: IdentityUpdate) -> Result<()>;

    fn decrypt_private_key(&self, passphrase: Option<&str>) -> Result<Vec<u8>>;

    fn refresh_cache(&mut self) -> Result<()>;
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

pub mod ffi {
    use crate::multipass::{
        identity::{FriendRequest, Identifier, Identity, IdentityUpdate, PublicKey},
        MultiPass,
    };
    use std::ffi::{c_void, CString};
    use std::os::raw::c_char;

    pub type MultiPassPointer = *mut c_void;
    pub type MultiPassBoxPointer = *mut Box<dyn MultiPass>;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_create_identity(
        ctx: MultiPassPointer,
        username: *mut c_char,
        passphrase: *mut c_char,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        let username = match username.is_null() {
            false => {
                let username = CString::from_raw(username).to_string_lossy().to_string();
                Some(username)
            }
            true => None,
        };

        let passphrase = match passphrase.is_null() {
            false => {
                let passphrase = CString::from_raw(passphrase).to_string_lossy().to_string();
                Some(passphrase)
            }
            true => None,
        };

        let mp = &mut *(ctx as MultiPassBoxPointer);
        (**mp)
            .create_identity(username.as_deref(), passphrase.as_deref())
            .is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_get_identity(
        ctx: MultiPassPointer,
        identifier: *mut Identifier,
    ) -> *mut Identity {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        if identifier.is_null() {
            return std::ptr::null_mut();
        }

        let mp = &*(ctx as MultiPassBoxPointer);
        let id = &*(identifier as *mut Identifier);
        match (**mp).get_identity(id.clone()) {
            Ok(identity) => Box::into_raw(Box::new(identity)) as *mut Identity,
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_get_own_identity(ctx: MultiPassPointer) -> *mut Identity {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        let mp = &*(ctx as MultiPassBoxPointer);
        match (**mp).get_own_identity() {
            Ok(identity) => Box::into_raw(Box::new(identity)) as *mut Identity,
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_update_identity(
        ctx: MultiPassPointer,
        option: *mut IdentityUpdate,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        let option = &*(option as *mut IdentityUpdate);
        (**mp).update_identity(option.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_decrypt_private_key(
        ctx: MultiPassPointer,
        passphrase: *mut c_char,
    ) -> *const u8 {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }
        let passphrase = match passphrase.is_null() {
            false => {
                let passphrase = CString::from_raw(passphrase).to_string_lossy().to_string();
                Some(passphrase)
            }
            true => None,
        };
        let mp = &mut *(ctx as MultiPassBoxPointer);
        match (**mp).decrypt_private_key(passphrase.as_deref()) {
            Ok(key) => key.as_ptr(),
            Err(_) => std::ptr::null_mut(),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_refresh_cache(ctx: MultiPassPointer) {
        if ctx.is_null() {
            return;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        if (**mp).refresh_cache().is_ok() {}
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_send_request(
        ctx: MultiPassPointer,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        let pk = &*pubkey;
        (**mp).send_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_accept_request(
        ctx: MultiPassPointer,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        let pk = &*pubkey;
        (**mp).send_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_deny_request(
        ctx: MultiPassPointer,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        let pk = &*pubkey;
        (**mp).send_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_close_request(
        ctx: MultiPassPointer,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        let pk = &*pubkey;
        (**mp).send_request(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_incoming_request(
        ctx: MultiPassPointer,
    ) -> *const *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        match (**mp).list_incoming_request() {
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
        ctx: MultiPassPointer,
    ) -> *const *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        match (**mp).list_outgoing_request() {
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
        ctx: MultiPassPointer,
    ) -> *const *const FriendRequest {
        if ctx.is_null() {
            return std::ptr::null();
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        match (**mp).list_all_request() {
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
        ctx: MultiPassPointer,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        let pk = &*pubkey;
        (**mp).remove_friend(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_block_key(
        ctx: MultiPassPointer,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        let pk = &*pubkey;
        (**mp).block_key(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_list_friends(
        ctx: MultiPassPointer,
    ) -> *const *const Identity {
        if ctx.is_null() {
            return std::ptr::null_mut();
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        match (**mp).list_friends() {
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
        ctx: MultiPassPointer,
        pubkey: *mut PublicKey,
    ) -> bool {
        if ctx.is_null() {
            return false;
        }

        if pubkey.is_null() {
            return false;
        }

        let mp = &mut *(ctx as MultiPassBoxPointer);
        let pk = &*pubkey;
        (**mp).has_friend(pk.clone()).is_ok()
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn multipass_free(ctx: MultiPassPointer) {
        let mp: Box<Box<dyn MultiPass>> = Box::from_raw(ctx as MultiPassBoxPointer);
        drop(mp)
    }
}

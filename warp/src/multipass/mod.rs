#![allow(clippy::result_large_err)]
pub mod generator;
pub mod identity;

use std::path::PathBuf;

use dyn_clone::DynClone;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::error::Error;

use crate::{Extension, SingleHandle};
use identity::Identity;

use crate::crypto::DID;
use crate::multipass::identity::{Identifier, IdentityUpdate};

use self::identity::{IdentityImage, IdentityProfile, IdentityStatus, Platform, Relationship};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    IdentityUpdate { did: DID },
    Blocked { did: DID },
    BlockedBy { did: DID },
    Unblocked { did: DID },
    UnblockedBy { did: DID },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportLocation<'a> {
    /// Remote location where the identity is stored
    Remote,

    /// Local path where the identity is stored
    Local { path: PathBuf },

    /// Buffer memory of where identity is stored
    Memory { buffer: &'a mut Vec<u8> },
}

#[derive(Debug, PartialEq, Eq)]
pub enum IdentityImportOption<'a> {
    Locate {
        /// Location of the identity
        location: ImportLocation<'a>,

        /// Passphrase of the identity
        passphrase: String,
    },
}

pub type MultiPassEventStream = BoxStream<'static, MultiPassEventKind>;

#[async_trait::async_trait]
pub trait MultiPass:
    Extension
    + IdentityInformation
    + MultiPassImportExport
    + Friends
    + MultiPassEvent
    + Sync
    + Send
    + SingleHandle
    + DynClone
{
    /// Create an [`Identity`]
    async fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<IdentityProfile, Error>;

    /// Obtain an [`Identity`] using [`Identifier`]
    async fn get_identity(&self, id: Identifier) -> Result<Vec<Identity>, Error>;

    /// Obtain your own [`Identity`]
    async fn get_own_identity(&self) -> Result<Identity, Error> {
        self.get_identity(Identifier::own())
            .await
            .and_then(|list| list.first().cloned().ok_or(Error::IdentityDoesntExist))
    }

    /// Update your own [`Identity`] using [`IdentityUpdate`]
    async fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error>;
}

dyn_clone::clone_trait_object!(MultiPass);

#[async_trait::async_trait]
pub trait MultiPassImportExport: Sync + Send {
    /// Import identity from a specific location
    async fn import_identity<'a>(
        &mut self,
        _: IdentityImportOption<'a>,
    ) -> Result<Identity, Error> {
        Err(Error::Unimplemented)
    }

    /// Manually export identity to a specific location
    async fn export_identity<'a>(&mut self, _: ImportLocation<'a>) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

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
    async fn list_incoming_request(&self) -> Result<Vec<DID>, Error> {
        Err(Error::Unimplemented)
    }

    /// Check to determine if a request been sent to the DID
    async fn sent_friend_request_to(&self, _: &DID) -> Result<bool, Error> {
        Err(Error::Unimplemented)
    }

    /// List the outgoing friend request
    async fn list_outgoing_request(&self) -> Result<Vec<DID>, Error> {
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
    async fn has_friend(&self, _: &DID) -> Result<bool, Error> {
        Err(Error::Unimplemented)
    }
}

#[async_trait::async_trait]
pub trait MultiPassEvent: Sync + Send {
    /// Subscribe to an stream of events
    async fn multipass_subscribe(&mut self) -> Result<MultiPassEventStream, Error> {
        Err(Error::Unimplemented)
    }
}

#[async_trait::async_trait]
pub trait IdentityInformation: Send + Sync {
    /// Profile picture belonging to the `Identity`
    async fn identity_picture(&self, _: &DID) -> Result<IdentityImage, Error> {
        Err(Error::Unimplemented)
    }

    /// Profile banner belonging to the `Identity`
    async fn identity_banner(&self, _: &DID) -> Result<IdentityImage, Error> {
        Err(Error::Unimplemented)
    }

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

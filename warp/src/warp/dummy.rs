use crate::constellation::directory::Directory;
use crate::constellation::{
    Constellation, ConstellationEvent, ConstellationEventStream, ConstellationProgressStream,
};
use crate::crypto::DID;
use crate::error::Error;
use crate::module::Module;
use crate::multipass::identity::{
    FriendRequest, Identifier, Identity, IdentityImage, IdentityProfile, IdentityStatus,
    IdentityUpdate, Platform, Relationship,
};
use crate::multipass::{
    Friends, GetIdentity, IdentityInformation, LocalIdentity, MultiPass, MultiPassEvent,
    MultiPassImportExport,
};
use crate::raygun::community::RayGunCommunity;
use crate::raygun::{
    Conversation, ConversationImage, EmbedState, GroupPermissions, Location, Message,
    MessageOptions, MessageReference, MessageStatus, Messages, PinState, RayGun, RayGunAttachment,
    RayGunConversationInformation, RayGunEvents, RayGunGroupConversation, RayGunStream,
    ReactionState,
};
use crate::tesseract::Tesseract;
use crate::{Extension, SingleHandle};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use futures::StreamExt;
use std::any::Any;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Copy, Clone, Debug, Default)]
pub struct Dummy;

impl SingleHandle for Dummy {
    fn handle(&self) -> Result<Box<dyn Any>, Error> {
        Err(Error::Unimplemented)
    }
}

impl Extension for Dummy {
    fn id(&self) -> String {
        "dummy".into()
    }

    fn name(&self) -> String {
        "Dummy".into()
    }

    fn description(&self) -> String {
        "Dummy extension to be a placeholder".into()
    }

    fn module(&self) -> Module {
        Module::Accounts
    }
}

#[async_trait::async_trait]
impl IdentityInformation for Dummy {
    async fn identity_picture(&self, _: &DID) -> Result<IdentityImage, Error> {
        Err(Error::Unimplemented)
    }

    async fn identity_banner(&self, _: &DID) -> Result<IdentityImage, Error> {
        Err(Error::Unimplemented)
    }

    async fn identity_status(&self, _: &DID) -> Result<IdentityStatus, Error> {
        Err(Error::Unimplemented)
    }

    async fn set_identity_status(&mut self, _: IdentityStatus) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn identity_relationship(&self, _: &DID) -> Result<Relationship, Error> {
        Err(Error::Unimplemented)
    }

    async fn identity_platform(&self, _: &DID) -> Result<Platform, Error> {
        Err(Error::Unimplemented)
    }
}

impl MultiPassImportExport for Dummy {}

#[async_trait::async_trait]
impl Friends for Dummy {
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
impl LocalIdentity for Dummy {
    async fn identity(&self) -> Result<Identity, Error> {
        Err(Error::Unimplemented)
    }

    async fn profile_picture(&self) -> Result<IdentityImage, Error> {
        Err(Error::Unimplemented)
    }

    async fn profile_banner(&self) -> Result<IdentityImage, Error> {
        Err(Error::Unimplemented)
    }

    async fn update_identity(&mut self, _: IdentityUpdate) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    fn tesseract(&self) -> Tesseract {
        Tesseract::new()
    }
}

impl MultiPassEvent for Dummy {}

#[async_trait::async_trait]
impl MultiPass for Dummy {
    async fn create_identity(
        &mut self,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<IdentityProfile, Error> {
        Err(Error::Unimplemented)
    }

    fn get_identity(&self, id: impl Into<Identifier>) -> GetIdentity {
        GetIdentity::new(id, futures::stream::empty().boxed())
    }
}

#[async_trait::async_trait]
impl ConstellationEvent for Dummy {
    async fn constellation_subscribe(&mut self) -> Result<ConstellationEventStream, Error> {
        Err(Error::Unimplemented)
    }
}

#[async_trait::async_trait]
impl Constellation for Dummy {
    fn modified(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn root_directory(&self) -> Directory {
        Directory::default()
    }

    fn current_size(&self) -> usize {
        0
    }

    fn max_size(&self) -> usize {
        0
    }

    fn select(&mut self, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    fn set_path(&mut self, _: PathBuf) {}

    fn get_path(&self) -> PathBuf {
        PathBuf::default()
    }

    fn go_back(&mut self) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    fn current_directory(&self) -> Result<Directory, Error> {
        Err(Error::Unimplemented)
    }

    fn open_directory(&self, _: &str) -> Result<Directory, Error> {
        Err(Error::Unimplemented)
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn put(&mut self, _: &str, _: &str) -> Result<ConstellationProgressStream, Error> {
        Err(Error::Unimplemented)
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn get(&self, _: &str, _: &str) -> Result<ConstellationProgressStream, Error> {
        Err(Error::Unimplemented)
    }

    async fn put_buffer(&mut self, _: &str, _: &[u8]) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn get_buffer(&self, _: &str) -> Result<Bytes, Error> {
        Err(Error::Unimplemented)
    }

    async fn put_stream(
        &mut self,
        _: &str,
        _: Option<usize>,
        _: BoxStream<'static, std::io::Result<Bytes>>,
    ) -> Result<ConstellationProgressStream, Error> {
        Err(Error::Unimplemented)
    }

    async fn get_stream(
        &self,
        _: &str,
    ) -> Result<BoxStream<'static, Result<Bytes, std::io::Error>>, Error> {
        Err(Error::Unimplemented)
    }

    async fn rename(&mut self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn remove(&mut self, _: &str, _: bool) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn move_item(&mut self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn create_directory(&mut self, _: &str, _: bool) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn sync_ref(&mut self, _: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

#[async_trait::async_trait]
impl RayGunStream for Dummy {}

#[async_trait::async_trait]
impl RayGunCommunity for Dummy {}

#[async_trait::async_trait]
impl RayGunGroupConversation for Dummy {}

#[async_trait::async_trait]
impl RayGunAttachment for Dummy {}

#[async_trait::async_trait]
impl RayGunEvents for Dummy {}

#[async_trait::async_trait]
impl RayGunConversationInformation for Dummy {
    async fn set_conversation_description(
        &mut self,
        _: Uuid,
        _: Option<&str>,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

#[async_trait::async_trait]
impl RayGun for Dummy {
    async fn create_conversation(&mut self, _: &DID) -> Result<Conversation, Error> {
        Err(Error::Unimplemented)
    }

    async fn create_group_conversation(
        &mut self,
        _: Option<String>,
        _: Vec<DID>,
        _: GroupPermissions,
    ) -> Result<Conversation, Error> {
        Err(Error::Unimplemented)
    }

    async fn get_conversation(&self, _: Uuid) -> Result<Conversation, Error> {
        Err(Error::Unimplemented)
    }

    async fn set_favorite_conversation(&mut self, _: Uuid, _: bool) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        Err(Error::Unimplemented)
    }

    async fn get_message(&self, _: Uuid, _: Uuid) -> Result<Message, Error> {
        Err(Error::Unimplemented)
    }

    async fn get_message_count(&self, _: Uuid) -> Result<usize, Error> {
        Err(Error::Unimplemented)
    }

    async fn message_status(&self, _: Uuid, _: Uuid) -> Result<MessageStatus, Error> {
        Err(Error::Unimplemented)
    }

    async fn get_message_references(
        &self,
        _: Uuid,
        _: MessageOptions,
    ) -> Result<BoxStream<'static, MessageReference>, Error> {
        Err(Error::Unimplemented)
    }

    async fn get_message_reference(&self, _: Uuid, _: Uuid) -> Result<MessageReference, Error> {
        Err(Error::Unimplemented)
    }

    async fn get_messages(&self, _: Uuid, _: MessageOptions) -> Result<Messages, Error> {
        Err(Error::Unimplemented)
    }

    async fn send(&mut self, _: Uuid, _: Vec<String>) -> Result<Uuid, Error> {
        Err(Error::Unimplemented)
    }

    async fn edit(&mut self, _: Uuid, _: Uuid, _: Vec<String>) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn delete(&mut self, _: Uuid, _: Option<Uuid>) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn react(&mut self, _: Uuid, _: Uuid, _: ReactionState, _: String) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn pin(&mut self, _: Uuid, _: Uuid, _: PinState) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn reply(&mut self, _: Uuid, _: Uuid, _: Vec<String>) -> Result<Uuid, Error> {
        Err(Error::Unimplemented)
    }

    async fn embeds(&mut self, _: Uuid, _: Uuid, _: EmbedState) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn update_conversation_permissions(
        &mut self,
        _: Uuid,
        _: GroupPermissions,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn conversation_icon(&self, _: Uuid) -> Result<ConversationImage, Error> {
        Err(Error::Unimplemented)
    }

    async fn conversation_banner(&self, _: Uuid) -> Result<ConversationImage, Error> {
        Err(Error::Unimplemented)
    }

    async fn update_conversation_icon(&mut self, _: Uuid, _: Location) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn update_conversation_banner(&mut self, _: Uuid, _: Location) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn remove_conversation_icon(&mut self, _: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn remove_conversation_banner(&mut self, _: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn archived_conversation(&mut self, _: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn unarchived_conversation(&mut self, _: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

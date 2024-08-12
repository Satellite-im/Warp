use crate::constellation::directory::Directory;
use crate::constellation::{
    Constellation, ConstellationEvent, ConstellationEventStream, ConstellationProgressStream,
};
use crate::crypto::DID;
use crate::error::Error;
use crate::module::Module;
use crate::multipass::identity::{
    Identifier, Identity, IdentityImage, IdentityProfile, IdentityStatus, IdentityUpdate, Platform,
    Relationship,
};
use crate::multipass::{
    Friends, GetIdentity, IdentityImportOption, IdentityInformation, ImportLocation, LocalIdentity,
    MultiPass, MultiPassEvent, MultiPassEventStream, MultiPassImportExport,
};
use crate::raygun::{
    AttachmentEventStream, Conversation, ConversationSettings, EmbedState, GroupSettings, Location,
    Message, MessageEvent, MessageEventStream, MessageOptions, MessageReference, MessageStatus,
    Messages, PinState, RayGun, RayGunAttachment, RayGunEventStream, RayGunEvents,
    RayGunGroupConversation, RayGunStream, ReactionState,
};
use crate::tesseract::Tesseract;
use crate::{Extension, SingleHandle};
use chrono::{DateTime, Utc};
use futures::stream::BoxStream;
use std::any::Any;
use std::path::PathBuf;
use uuid::Uuid;

pub struct Warp<M, R, C>
where
    M: MultiPass,
    R: RayGun,
    C: Constellation,
{
    multipass: M,
    raygun: R,
    constellation: C,
}

impl<M, R, C> Warp<M, R, C>
where
    M: MultiPass + Clone,
    R: RayGun + Clone,
    C: Constellation + Clone,
{
    pub fn new(multipass: &M, raygun: &R, constellation: &C) -> Self {
        Self {
            multipass: multipass.clone(),
            raygun: raygun.clone(),
            constellation: constellation.clone(),
        }
    }
}

impl<M, R, C> Warp<M, R, C>
where
    M: MultiPass,
    R: RayGun,
    C: Constellation,
{
    pub fn from(multipass: M, raygun: R, constellation: C) -> Self {
        Self {
            multipass,
            raygun,
            constellation,
        }
    }
}

impl<M, R, C> Clone for Warp<M, R, C>
where
    M: MultiPass + Clone,
    R: RayGun + Clone,
    C: Constellation + Clone,
{
    fn clone(&self) -> Self {
        Self {
            multipass: self.multipass.clone(),
            raygun: self.raygun.clone(),
            constellation: self.constellation.clone(),
        }
    }
}

impl<M, R, C> Warp<M, R, C>
where
    M: MultiPass,
    R: RayGun,
    C: Constellation,
{
    pub fn multipass(&self) -> &M {
        &self.multipass
    }

    pub fn raygun(&self) -> &R {
        &self.raygun
    }

    pub fn constellation(&self) -> &C {
        &self.constellation
    }

    pub fn multipass_mut(&mut self) -> &mut M {
        &mut self.multipass
    }

    pub fn raygun_mut(&mut self) -> &mut R {
        &mut self.raygun
    }

    pub fn constellation_mut(&mut self) -> &mut C {
        &mut self.constellation
    }
}

impl<M, R, C> Extension for Warp<M, R, C>
where
    M: MultiPass,
    R: RayGun,
    C: Constellation,
{
    fn id(&self) -> String {
        self.multipass.id()
    }

    fn name(&self) -> String {
        self.multipass.name()
    }

    fn module(&self) -> Module {
        self.multipass.module()
    }
}

impl<M, R, C> SingleHandle for Warp<M, R, C>
where
    M: MultiPass,
    R: RayGun,
    C: Constellation,
{
    fn handle(&self) -> Result<Box<dyn Any>, Error> {
        self.multipass.handle()
    }
}

#[async_trait::async_trait]
impl<M, R, C> IdentityInformation for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    /// Profile picture belonging to the `Identity`
    async fn identity_picture(&self, identity: &DID) -> Result<IdentityImage, Error> {
        self.multipass.identity_picture(identity).await
    }

    /// Profile banner belonging to the `Identity`
    async fn identity_banner(&self, identity: &DID) -> Result<IdentityImage, Error> {
        self.multipass.identity_banner(identity).await
    }

    /// Identity status to determine if they are online or offline
    async fn identity_status(&self, identity: &DID) -> Result<IdentityStatus, Error> {
        self.multipass.identity_status(identity).await
    }

    /// Identity status to determine if they are online or offline
    async fn set_identity_status(&mut self, status: IdentityStatus) -> Result<(), Error> {
        self.multipass.set_identity_status(status).await
    }

    /// Find the relationship with an existing identity.
    async fn identity_relationship(&self, identity: &DID) -> Result<Relationship, Error> {
        self.multipass.identity_relationship(identity).await
    }

    /// Returns the identity platform while online.
    async fn identity_platform(&self, identity: &DID) -> Result<Platform, Error> {
        self.multipass.identity_platform(identity).await
    }
}

#[async_trait::async_trait]
impl<M, R, C> MultiPassImportExport for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    /// Import identity from a specific location
    async fn import_identity<'a>(
        &mut self,
        option: IdentityImportOption<'a>,
    ) -> Result<Identity, Error> {
        self.multipass.import_identity(option).await
    }

    /// Manually export identity to a specific location
    async fn export_identity<'a>(&mut self, location: ImportLocation<'a>) -> Result<(), Error> {
        self.multipass.export_identity(location).await
    }
}

#[async_trait::async_trait]
impl<M, R, C> Friends for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    async fn send_request(&mut self, identity: &DID) -> Result<(), Error> {
        self.multipass.send_request(identity).await
    }

    /// Accept friend request from public key
    async fn accept_request(&mut self, identity: &DID) -> Result<(), Error> {
        self.multipass.accept_request(identity).await
    }

    /// Deny friend request from public key
    async fn deny_request(&mut self, identity: &DID) -> Result<(), Error> {
        self.multipass.deny_request(identity).await
    }

    /// Closing or retracting friend request
    async fn close_request(&mut self, identity: &DID) -> Result<(), Error> {
        self.multipass.close_request(identity).await
    }

    /// Check to determine if a request been received from the DID
    async fn received_friend_request_from(&self, identity: &DID) -> Result<bool, Error> {
        self.multipass.received_friend_request_from(identity).await
    }

    /// List the incoming friend request
    async fn list_incoming_request(&self) -> Result<Vec<DID>, Error> {
        self.multipass.list_incoming_request().await
    }

    /// Check to determine if a request been sent to the DID
    async fn sent_friend_request_to(&self, identity: &DID) -> Result<bool, Error> {
        self.multipass.sent_friend_request_to(identity).await
    }

    /// List the outgoing friend request
    async fn list_outgoing_request(&self) -> Result<Vec<DID>, Error> {
        self.multipass.list_outgoing_request().await
    }

    /// Remove friend from contacts
    async fn remove_friend(&mut self, identity: &DID) -> Result<(), Error> {
        self.multipass.remove_friend(identity).await
    }

    /// Block public key, rather it be a friend or not, from being able to send request to account public address
    async fn block(&mut self, identity: &DID) -> Result<(), Error> {
        self.multipass.block(identity).await
    }

    /// Unblock public key
    async fn unblock(&mut self, identity: &DID) -> Result<(), Error> {
        self.multipass.unblock(identity).await
    }

    /// List block list
    async fn block_list(&self) -> Result<Vec<DID>, Error> {
        self.multipass.block_list().await
    }

    /// Check to see if public key is blocked
    async fn is_blocked(&self, identity: &DID) -> Result<bool, Error> {
        self.multipass.is_blocked(identity).await
    }

    /// List all friends public key
    async fn list_friends(&self) -> Result<Vec<DID>, Error> {
        self.multipass.list_friends().await
    }

    /// Check to see if public key is friend of the account
    async fn has_friend(&self, identity: &DID) -> Result<bool, Error> {
        self.multipass.has_friend(identity).await
    }
}

#[async_trait::async_trait]
impl<M, R, C> LocalIdentity for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    async fn identity(&self) -> Result<Identity, Error> {
        self.multipass.identity().await
    }

    async fn profile_picture(&self) -> Result<IdentityImage, Error> {
        self.multipass.profile_picture().await
    }

    async fn profile_banner(&self) -> Result<IdentityImage, Error> {
        self.multipass.profile_banner().await
    }

    async fn update_identity(&mut self, option: IdentityUpdate) -> Result<(), Error> {
        self.multipass.update_identity(option).await
    }

    fn tesseract(&self) -> Tesseract {
        self.multipass.tesseract()
    }
}

#[async_trait::async_trait]
impl<M, R, C> MultiPassEvent for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    async fn multipass_subscribe(&mut self) -> Result<MultiPassEventStream, Error> {
        self.multipass.multipass_subscribe().await
    }
}

#[async_trait::async_trait]
impl<M, R, C> MultiPass for Warp<M, R, C>
where
    M: MultiPass,
    R: RayGun,
    C: Constellation,
{
    async fn create_identity(
        &mut self,
        username: Option<&str>,
        passphrase: Option<&str>,
    ) -> Result<IdentityProfile, Error> {
        self.multipass.create_identity(username, passphrase).await
    }

    fn get_identity(&self, id: impl Into<Identifier>) -> GetIdentity {
        self.multipass.get_identity(id)
    }
}

#[async_trait::async_trait]
impl<M, R, C> Constellation for Warp<M, R, C>
where
    M: MultiPass,
    R: RayGun,
    C: Constellation,
{
    fn modified(&self) -> DateTime<Utc> {
        self.constellation.modified()
    }

    fn root_directory(&self) -> Directory {
        self.constellation.root_directory()
    }

    fn current_size(&self) -> usize {
        self.constellation.current_size()
    }

    fn max_size(&self) -> usize {
        self.constellation.max_size()
    }

    fn select(&mut self, path: &str) -> Result<(), Error> {
        self.constellation.select(path)
    }

    fn set_path(&mut self, path: PathBuf) {
        self.constellation.set_path(path)
    }

    fn get_path(&self) -> PathBuf {
        self.constellation.get_path()
    }

    fn go_back(&mut self) -> Result<(), Error> {
        self.constellation.go_back()
    }

    fn current_directory(&self) -> Result<Directory, Error> {
        self.constellation.current_directory()
    }

    fn open_directory(&self, path: &str) -> Result<Directory, Error> {
        self.constellation.open_directory(path)
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn put(&mut self, name: &str, path: &str) -> Result<ConstellationProgressStream, Error> {
        self.constellation.put(name, path).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn get(&self, name: &str, path: &str) -> Result<ConstellationProgressStream, Error> {
        self.constellation.get(name, path).await
    }

    async fn put_buffer(&mut self, name: &str, buffer: &[u8]) -> Result<(), Error> {
        self.constellation.put_buffer(name, buffer).await
    }

    async fn get_buffer(&self, name: &str) -> Result<Vec<u8>, Error> {
        self.constellation.get_buffer(name).await
    }

    async fn put_stream(
        &mut self,
        name: &str,
        total_size: Option<usize>,
        stream: BoxStream<'static, std::io::Result<Vec<u8>>>,
    ) -> Result<ConstellationProgressStream, Error> {
        self.constellation
            .put_stream(name, total_size, stream)
            .await
    }

    async fn get_stream(
        &self,
        name: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, std::io::Error>>, Error> {
        self.constellation.get_stream(name).await
    }

    async fn rename(&mut self, current: &str, new: &str) -> Result<(), Error> {
        self.constellation.rename(current, new).await
    }

    async fn remove(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        self.constellation.remove(name, recursive).await
    }

    async fn move_item(&mut self, from: &str, to: &str) -> Result<(), Error> {
        self.constellation.move_item(from, to).await
    }

    async fn create_directory(&mut self, name: &str, recursive: bool) -> Result<(), Error> {
        self.constellation.create_directory(name, recursive).await
    }

    async fn sync_ref(&mut self, name: &str) -> Result<(), Error> {
        self.constellation.sync_ref(name).await
    }
}

#[async_trait::async_trait]
impl<M, R, C> ConstellationEvent for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    async fn constellation_subscribe(&mut self) -> Result<ConstellationEventStream, Error> {
        self.constellation.constellation_subscribe().await
    }
}

#[async_trait::async_trait]
impl<M, R, C> RayGunStream for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    async fn get_conversation_stream(
        &mut self,
        conversation_id: Uuid,
    ) -> Result<MessageEventStream, Error> {
        self.raygun.get_conversation_stream(conversation_id).await
    }

    async fn raygun_subscribe(&mut self) -> Result<RayGunEventStream, Error> {
        self.raygun.raygun_subscribe().await
    }
}

#[async_trait::async_trait]
impl<M, R, C> RayGunGroupConversation for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    async fn update_conversation_name(
        &mut self,
        conversation_id: Uuid,
        name: &str,
    ) -> Result<(), Error> {
        self.raygun
            .update_conversation_name(conversation_id, name)
            .await
    }

    async fn add_recipient(&mut self, conversation_id: Uuid, identity: &DID) -> Result<(), Error> {
        self.raygun.add_recipient(conversation_id, identity).await
    }

    async fn remove_recipient(
        &mut self,
        conversation_id: Uuid,
        identity: &DID,
    ) -> Result<(), Error> {
        self.raygun
            .remove_recipient(conversation_id, identity)
            .await
    }
}

#[async_trait::async_trait]
impl<M, R, C> RayGunAttachment for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    async fn attach(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
        locations: Vec<Location>,
        message: Vec<String>,
    ) -> Result<(Uuid, AttachmentEventStream), Error> {
        self.raygun
            .attach(conversation_id, message_id, locations, message)
            .await
    }

    async fn download(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        name: String,
        path: PathBuf,
    ) -> Result<ConstellationProgressStream, Error> {
        self.raygun
            .download(conversation_id, message_id, name, path)
            .await
    }

    async fn download_stream(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
        name: &str,
    ) -> Result<BoxStream<'static, Result<Vec<u8>, std::io::Error>>, Error> {
        self.raygun
            .download_stream(conversation_id, message_id, name)
            .await
    }
}

#[async_trait::async_trait]
impl<M, R, C> RayGunEvents for Warp<M, R, C>
where
    C: Constellation,
    M: MultiPass,
    R: RayGun,
{
    async fn send_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        self.raygun.send_event(conversation_id, event).await
    }

    async fn cancel_event(
        &mut self,
        conversation_id: Uuid,
        event: MessageEvent,
    ) -> Result<(), Error> {
        self.raygun.cancel_event(conversation_id, event).await
    }
}

#[async_trait::async_trait]
impl<M, R, C> RayGun for Warp<M, R, C>
where
    M: MultiPass,
    R: RayGun,
    C: Constellation,
{
    async fn create_conversation(&mut self, identity: &DID) -> Result<Conversation, Error> {
        self.raygun.create_conversation(identity).await
    }

    async fn create_group_conversation(
        &mut self,
        name: Option<String>,
        members: Vec<DID>,
        settings: GroupSettings,
    ) -> Result<Conversation, Error> {
        self.raygun
            .create_group_conversation(name, members, settings)
            .await
    }

    async fn get_conversation(&self, conversation_id: Uuid) -> Result<Conversation, Error> {
        self.raygun.get_conversation(conversation_id).await
    }

    async fn set_favorite_conversation(
        &mut self,
        conversation_id: Uuid,
        favorite: bool,
    ) -> Result<(), Error> {
        self.raygun
            .set_favorite_conversation(conversation_id, favorite)
            .await
    }

    async fn list_conversations(&self) -> Result<Vec<Conversation>, Error> {
        self.raygun.list_conversations().await
    }

    async fn get_message(&self, conversation_id: Uuid, message_id: Uuid) -> Result<Message, Error> {
        self.raygun.get_message(conversation_id, message_id).await
    }

    async fn get_message_count(&self, conversation_id: Uuid) -> Result<usize, Error> {
        self.raygun.get_message_count(conversation_id).await
    }

    async fn message_status(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageStatus, Error> {
        self.raygun
            .message_status(conversation_id, message_id)
            .await
    }

    async fn get_message_references(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
    ) -> Result<BoxStream<'static, MessageReference>, Error> {
        self.raygun
            .get_message_references(conversation_id, options)
            .await
    }

    async fn get_message_reference(
        &self,
        conversation_id: Uuid,
        message_id: Uuid,
    ) -> Result<MessageReference, Error> {
        self.raygun
            .get_message_reference(conversation_id, message_id)
            .await
    }

    async fn get_messages(
        &self,
        conversation_id: Uuid,
        options: MessageOptions,
    ) -> Result<Messages, Error> {
        self.raygun.get_messages(conversation_id, options).await
    }

    async fn send(&mut self, conversation_id: Uuid, message: Vec<String>) -> Result<Uuid, Error> {
        self.raygun.send(conversation_id, message).await
    }

    async fn edit(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> Result<(), Error> {
        self.raygun.edit(conversation_id, message_id, message).await
    }

    async fn delete(
        &mut self,
        conversation_id: Uuid,
        message_id: Option<Uuid>,
    ) -> Result<(), Error> {
        self.raygun.delete(conversation_id, message_id).await
    }

    async fn react(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: ReactionState,
        emoji: String,
    ) -> Result<(), Error> {
        self.raygun
            .react(conversation_id, message_id, state, emoji)
            .await
    }

    async fn pin(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        state: PinState,
    ) -> Result<(), Error> {
        self.raygun.pin(conversation_id, message_id, state).await
    }

    async fn reply(
        &mut self,
        conversation_id: Uuid,
        message_id: Uuid,
        message: Vec<String>,
    ) -> Result<Uuid, Error> {
        self.raygun
            .reply(conversation_id, message_id, message)
            .await
    }

    //TODO: Remove
    async fn embeds(&mut self, _: Uuid, _: Uuid, _: EmbedState) -> Result<(), Error> {
        unreachable!()
    }

    async fn update_conversation_settings(
        &mut self,
        conversation_id: Uuid,
        settings: ConversationSettings,
    ) -> Result<(), Error> {
        self.raygun
            .update_conversation_settings(conversation_id, settings)
            .await
    }

    async fn archived_conversation(&mut self, conversation_id: Uuid) -> Result<(), Error> {
        self.raygun.archived_conversation(conversation_id).await
    }

    async fn unarchived_conversation(&mut self, conversation_id: Uuid) -> Result<(), Error> {
        self.raygun.unarchived_conversation(conversation_id).await
    }
}

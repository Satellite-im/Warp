use chrono::{DateTime, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::DID;
use crate::raygun::{Error, Location};

use super::{ConversationImage, MessageEventStream};

pub type RoleId = Uuid;
pub type CommunityRoles = IndexMap<RoleId, CommunityRole>;
pub type CommunityPermissions = IndexMap<CommunityPermission, IndexSet<RoleId>>;
pub type CommunityChannelPermissions = IndexMap<CommunityChannelPermission, IndexSet<RoleId>>;

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommunityRole {
    id: RoleId,
    name: String,
    members: IndexSet<DID>,
}
impl CommunityRole {
    pub fn id(&self) -> RoleId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn members(&self) -> &IndexSet<DID> {
        &self.members
    }
}
impl CommunityRole {
    pub fn set_id(&mut self, id: RoleId) {
        self.id = id;
    }
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }
    pub fn set_members(&mut self, members: IndexSet<DID>) {
        self.members = members;
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommunityInvite {
    id: Uuid,
    target_user: Option<DID>,
    created: DateTime<Utc>,
    expiry: Option<DateTime<Utc>>,
}
impl CommunityInvite {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn target_user(&self) -> Option<&DID> {
        self.target_user.as_ref()
    }
    pub fn created(&self) -> DateTime<Utc> {
        self.created
    }
    pub fn expiry(&self) -> Option<DateTime<Utc>> {
        self.expiry
    }
}
impl CommunityInvite {
    pub fn set_id(&mut self, id: Uuid) {
        self.id = id;
    }
    pub fn set_target_user(&mut self, target_user: Option<DID>) {
        self.target_user = target_user;
    }
    pub fn set_created(&mut self, created: DateTime<Utc>) {
        self.created = created;
    }
    pub fn set_expiry(&mut self, expiry: Option<DateTime<Utc>>) {
        self.expiry = expiry;
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Community {
    id: Uuid,
    name: String,
    description: Option<String>,
    creator: DID,
    created: DateTime<Utc>,
    modified: DateTime<Utc>,
    members: IndexSet<DID>,
    channels: IndexSet<Uuid>,
    roles: IndexSet<RoleId>,
    permissions: CommunityPermissions,
    invites: IndexSet<Uuid>,
}
impl Community {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn creator(&self) -> &DID {
        &self.creator
    }
    pub fn created(&self) -> DateTime<Utc> {
        self.created
    }
    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }
    pub fn members(&self) -> &IndexSet<DID> {
        &self.members
    }
    pub fn channels(&self) -> &IndexSet<Uuid> {
        &self.channels
    }
    pub fn roles(&self) -> &IndexSet<RoleId> {
        &self.roles
    }
    pub fn permissions(&self) -> &CommunityPermissions {
        &self.permissions
    }
    pub fn invites(&self) -> &IndexSet<Uuid> {
        &self.invites
    }
}
impl Community {
    pub fn set_id(&mut self, id: Uuid) {
        self.id = id;
    }
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }
    pub fn set_description(&mut self, description: Option<String>) {
        self.description = description;
    }
    pub fn set_creator(&mut self, creator: DID) {
        self.creator = creator;
    }
    pub fn set_created(&mut self, created: DateTime<Utc>) {
        self.created = created;
    }
    pub fn set_modified(&mut self, modified: DateTime<Utc>) {
        self.modified = modified;
    }
    pub fn set_members(&mut self, members: IndexSet<DID>) {
        self.members = members;
    }
    pub fn set_channels(&mut self, channels: IndexSet<Uuid>) {
        self.channels = channels;
    }
    pub fn set_roles(&mut self, roles: IndexSet<RoleId>) {
        self.roles = roles;
    }
    pub fn set_permissions(&mut self, permissions: CommunityPermissions) {
        self.permissions = permissions;
    }
    pub fn set_invites(&mut self, invites: IndexSet<Uuid>) {
        self.invites = invites;
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommunityChannel {
    id: Uuid,
    name: String,
    description: Option<String>,
    created: DateTime<Utc>,
    modified: DateTime<Utc>,
    channel_type: CommunityChannelType,
    permissions: CommunityChannelPermissions,
}

impl CommunityChannel {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
    pub fn created(&self) -> DateTime<Utc> {
        self.created
    }
    pub fn modified(&self) -> DateTime<Utc> {
        self.modified
    }
    pub fn channel_type(&self) -> CommunityChannelType {
        self.channel_type
    }
    pub fn permissions(&self) -> &CommunityChannelPermissions {
        &self.permissions
    }
}
impl CommunityChannel {
    pub fn set_id(&mut self, id: Uuid) {
        self.id = id;
    }
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }
    pub fn set_description(&mut self, description: Option<String>) {
        self.description = description;
    }
    pub fn set_created(&mut self, created: DateTime<Utc>) {
        self.created = created;
    }
    pub fn set_modified(&mut self, modified: DateTime<Utc>) {
        self.modified = modified;
    }
    pub fn set_channel_type(&mut self, channel_type: CommunityChannelType) {
        self.channel_type = channel_type;
    }
    pub fn set_permissions(&mut self, permissions: CommunityChannelPermissions) {
        self.permissions = permissions;
    }
}

#[derive(Default, Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommunityChannelType {
    #[default]
    Standard,
    VoiceEnabled,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CommunityPermission {
    EditName,
    EditDescription,
    EditIcon,
    EditBanner,

    CreateRoles,
    EditRoles,
    DeleteRoles,

    GrantRoles,
    RevokeRoles,

    GrantPermissions,
    RevokePermissions,

    CreateInvites,
    EditInvites,
    DeleteInvites,

    CreateChannels,
    EditChannels,
    DeleteChannels,

    RemoveMembers,

    DeleteMessages,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CommunityChannelPermission {
    ViewChannel,
    SendMessages,
}

#[async_trait::async_trait]
pub trait RayGunCommunity: Sync + Send {
    async fn get_community_stream(
        &mut self,
        _community_id: Uuid,
    ) -> Result<MessageEventStream, Error> {
        Err(Error::Unimplemented)
    }

    async fn create_community(&mut self, _name: &str) -> Result<Community, Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_community(&mut self, _community_id: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn get_community(&self, _community_id: Uuid) -> Result<Community, Error> {
        Err(Error::Unimplemented)
    }

    async fn list_communities_joined(&self) -> Result<IndexSet<Uuid>, Error> {
        Err(Error::Unimplemented)
    }
    async fn list_communities_invited_to(&self) -> Result<Vec<(Uuid, CommunityInvite)>, Error> {
        Err(Error::Unimplemented)
    }
    async fn leave_community(&mut self, _community_id: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn get_community_icon(&self, _community_id: Uuid) -> Result<ConversationImage, Error> {
        Err(Error::Unimplemented)
    }
    async fn get_community_banner(&self, _community_id: Uuid) -> Result<ConversationImage, Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_community_icon(
        &mut self,
        _community_id: Uuid,
        _location: Location,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_community_banner(
        &mut self,
        _community_id: Uuid,
        _location: Location,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn create_community_invite(
        &mut self,
        _community_id: Uuid,
        _target_user: Option<DID>,
        _expiry: Option<DateTime<Utc>>,
    ) -> Result<CommunityInvite, Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_community_invite(
        &mut self,
        _community_id: Uuid,
        _invite_id: Uuid,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn get_community_invite(
        &self,
        _community_id: Uuid,
        _invite_id: Uuid,
    ) -> Result<CommunityInvite, Error> {
        Err(Error::Unimplemented)
    }
    async fn accept_community_invite(
        &mut self,
        _community_id: Uuid,
        _invite_id: Uuid,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_community_invite(
        &mut self,
        _community_id: Uuid,
        _invite_id: Uuid,
        _invite: CommunityInvite,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn create_community_role(
        &mut self,
        _community_id: Uuid,
        _name: &str,
    ) -> Result<CommunityRole, Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_community_role(
        &mut self,
        _community_id: Uuid,
        _role_id: RoleId,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn get_community_role(
        &mut self,
        _community_id: Uuid,
        _role_id: RoleId,
    ) -> Result<CommunityRole, Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_community_role_name(
        &mut self,
        _community_id: Uuid,
        _role_id: RoleId,
        _new_name: String,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn grant_community_role(
        &mut self,
        _community_id: Uuid,
        _role_id: RoleId,
        _user: DID,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn revoke_community_role(
        &mut self,
        _community_id: Uuid,
        _role_id: RoleId,
        _user: DID,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn create_community_channel(
        &mut self,
        _community_id: Uuid,
        _channel_name: &str,
        _channel_type: CommunityChannelType,
    ) -> Result<CommunityChannel, Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_community_channel(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn get_community_channel(
        &self,
        _community_id: Uuid,
        _channel_id: Uuid,
    ) -> Result<CommunityChannel, Error> {
        Err(Error::Unimplemented)
    }

    async fn edit_community_name(&mut self, _community_id: Uuid, _name: &str) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_community_description(
        &mut self,
        _community_id: Uuid,
        _description: Option<String>,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn grant_community_permission(
        &mut self,
        _community_id: Uuid,
        _permission: CommunityPermission,
        _role_id: RoleId,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn revoke_community_permission(
        &mut self,
        _community_id: Uuid,
        _permission: CommunityPermission,
        _role_id: RoleId,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn grant_community_permission_for_all(
        &mut self,
        _community_id: Uuid,
        _permission: CommunityPermission,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn revoke_community_permission_for_all(
        &mut self,
        _community_id: Uuid,
        _permission: CommunityPermission,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn remove_community_member(
        &mut self,
        _community_id: Uuid,
        _member: DID,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn edit_community_channel_name(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _name: &str,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_community_channel_description(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _description: Option<String>,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn grant_community_channel_permission(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _permission: CommunityChannelPermission,
        _role_id: RoleId,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn revoke_community_channel_permission(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _permission: CommunityChannelPermission,
        _role_id: RoleId,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn grant_community_channel_permission_for_all(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _permission: CommunityChannelPermission,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn revoke_community_channel_permission_for_all(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _permission: CommunityChannelPermission,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn send_community_channel_message(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _message: &str,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_community_channel_message(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _message_id: Uuid,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

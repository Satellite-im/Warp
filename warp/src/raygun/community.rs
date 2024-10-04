use chrono::{DateTime, Utc};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::DID;
use crate::raygun::Error;

pub type Role = String;
pub type CommunityPermissions = IndexMap<CommunityPermission, IndexSet<Role>>;
pub type CommunityChannelPermissions = IndexMap<CommunityChannelPermission, IndexSet<Role>>;

pub struct CommunityInvite {
    target_user: Option<DID>,
    created: DateTime<Utc>,
    expiry: Option<DateTime<Utc>>,
}
impl CommunityInvite {
    pub fn target_user(&self) -> Option<DID> {
        self.target_user
    }
    pub fn created(&self) -> DateTime<Utc> {
        self.created
    }
    pub fn expiry(&self) -> Option<DateTime<Utc>> {
        self.expiry
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Community {
    id: Uuid,
    name: String,
    description: Option<String>,
    creator: DID,
    created: DateTime<Utc>,
    modified: DateTime<Utc>,
    members: IndexSet<DID>,
    channels: IndexSet<Uuid>,
    roles: IndexSet<Role>,
    permissions: CommunityPermissions,
    invites: IndexSet<Uuid>,
}
impl Community {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn name(&self) -> &String {
        &self.name
    }
    pub fn description(&self) -> &Option<String> {
        &self.description
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
    pub fn roles(&self) -> &IndexSet<Role> {
        &self.roles
    }
    pub fn permissions(&self) -> &CommunityPermissions {
        &self.permissions
    }
    pub fn invites(&self) -> &IndexSet<Uuid> {
        &self.invites
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub fn name(&self) -> &String {
        &self.name
    }
    pub fn description(&self) -> &Option<String> {
        &self.description
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
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum CommunityChannelType {
    Standard,
    VoiceEnabled,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CommunityPermission {
    EditName,
    EditDescription,
    ManageRoles,
    ManagePermissions,
    ManageMembers,
    ManageChannels,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CommunityChannelPermission {
    ViewChannel,
    EditName,
    EditDescription,
    SendMessages,
    DeleteMessages,
}

#[async_trait::async_trait]
pub trait RayGunCommunity: Sync + Send {
    async fn create_community(&mut self, _name: &str) -> Result<Community, Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_community(&mut self, _community_id: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn get_community(&mut self, _community_id: Uuid) -> Result<Community, Error> {
        Err(Error::Unimplemented)
    }

    async fn create_invite(
        &mut self,
        _community_id: Uuid,
        _invite: CommunityInvite,
    ) -> Result<Uuid, Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_invite(&mut self, _community_id: Uuid, _invite_id: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn get_invite(
        &mut self,
        _community_id: Uuid,
        _invite_id: Uuid,
    ) -> Result<CommunityInvite, Error> {
        Err(Error::Unimplemented)
    }
    async fn accept_invite(&mut self, _community_id: Uuid, _invite_id: Uuid) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    async fn create_channel(
        &mut self,
        _community_id: Uuid,
        _channel_name: &str,
        _channel_type: CommunityChannelType,
    ) -> Result<CommunityChannel, Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_channel(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn get_channel(
        &mut self,
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
    async fn edit_community_roles(
        &mut self,
        _community_id: Uuid,
        _roles: IndexSet<Role>,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_community_permissions(
        &mut self,
        _community_id: Uuid,
        _permissions: CommunityPermissions,
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

    async fn edit_channel_name(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _name: &str,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_channel_description(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _description: Option<String>,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn edit_channel_permissions(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _permissions: CommunityChannelPermissions,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn send_channel_message(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _message: &str,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
    async fn delete_channel_message(
        &mut self,
        _community_id: Uuid,
        _channel_id: Uuid,
        _message_id: Uuid,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }
}

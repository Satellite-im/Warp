use crate::crypto::{DID};
use crate::error::Error;
use crate::raygun::Uid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Copy, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub enum GroupStatus {
    Opened,
    Closed,
}

impl Default for GroupStatus {
    fn default() -> Self {
        Self::Closed
    }
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct GroupId(Uid);

impl GroupId {
    pub fn new_uuid() -> GroupId {
        Self(Uid::new_uuid())
    }

    pub fn from_id(id: Uuid) -> GroupId {
        Self(Uid::Id(id))
    }

    pub fn from_did_key(pubkey: DID) -> GroupId {
        Self(Uid::DIDKey(pubkey))
    }

    pub fn get_id(&self) -> Option<Uuid> {
        match &self.0 {
            Uid::Id(id) => Some(*id),
            Uid::DIDKey(_) => None,
        }
    }

    pub fn get_public_key(&self) -> Option<DID> {
        match &self.0 {
            Uid::Id(_) => None,
            Uid::DIDKey(k) => Some(k.clone()),
        }
    }
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct GroupMember(Uid);

impl GroupMember {
    pub fn new_uuid() -> GroupMember {
        Self(Uid::new_uuid())
    }

    pub fn from_id(id: Uuid) -> GroupMember {
        Self(Uid::Id(id))
    }

    pub fn from_did_key(pubkey: DID) -> GroupMember {
        Self(Uid::DIDKey(pubkey))
    }

    pub fn get_id(&self) -> Option<Uuid> {
        match &self.0 {
            Uid::Id(id) => Some(*id),
            Uid::DIDKey(_) => None,
        }
    }

    pub fn get_public_key(&self) -> Option<DID> {
        match &self.0 {
            Uid::Id(_) => None,
            Uid::DIDKey(k) => Some(k.clone()),
        }
    }
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Group {
    id: GroupId,
    name: String,
    creator: GroupMember,
    admin: GroupMember,
    members: u32,
    status: GroupStatus,
}

impl Group {
    pub fn id(&self) -> GroupId {
        self.id.clone()
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn creator(&self) -> GroupMember {
        self.creator.clone()
    }

    pub fn admin(&self) -> GroupMember {
        self.admin.clone()
    }

    pub fn members(&self) -> u32 {
        self.members
    }

    pub fn status(&self) -> GroupStatus {
        self.status
    }
}

impl Group {
    pub fn set_id(&mut self, id: GroupId) {
        self.id = id;
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub fn set_creator(&mut self, creator: GroupMember) {
        self.creator = creator;
    }

    pub fn set_admin(&mut self, admin: GroupMember) {
        self.admin = admin;
    }

    pub fn set_members(&mut self, members: u32) {
        self.members = members;
    }

    pub fn set_status(&mut self, status: GroupStatus) {
        self.status = status;
    }
}

#[derive(Default, Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct GroupInvitation {
    id: Uuid,
    group: GroupId,
    sender: GroupMember,
    recipient: GroupMember,
    #[serde(flatten)]
    metadata: HashMap<String, serde_json::Value>,
}

impl GroupInvitation {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn group(&self) -> GroupId {
        self.group.clone()
    }

    pub fn sender(&self) -> GroupMember {
        self.sender.clone()
    }

    pub fn recipient(&self) -> GroupMember {
        self.recipient.clone()
    }

    pub fn metadata(&self) -> HashMap<String, serde_json::Value> {
        self.metadata.clone()
    }
}

impl GroupInvitation {
    pub fn set_id(&mut self, id: Uuid) {
        self.id = id;
    }

    pub fn set_group(&mut self, group: GroupId) {
        self.group = group;
    }

    pub fn set_sender(&mut self, sender: GroupMember) {
        self.sender = sender;
    }

    pub fn set_recipient(&mut self, recipient: GroupMember) {
        self.recipient = recipient;
    }

    pub fn set_metadata(&mut self, metadata: HashMap<String, serde_json::Value>) {
        self.metadata = metadata
    }
}

// General/Base GroupChat Trait
pub trait GroupChat: GroupInvite + GroupChatManagement {
    /// Join a existing group
    fn join_group(&mut self, id: GroupId) -> Result<(), Error>;

    /// Leave a group
    fn leave_group(&mut self, id: GroupId) -> Result<(), Error>;

    /// List members of group
    fn list_members(&self, id: GroupId) -> Result<Vec<GroupMember>, Error>;
}

// Group Invite Management Trait
pub trait GroupInvite {
    /// Sends a invite to join a group
    fn send_invite(&mut self, id: GroupId, recipient: GroupMember) -> Result<(), Error>;

    /// Accepts an invite to a group
    fn accept_invite(&mut self, id: GroupId) -> Result<(), Error>;

    /// Dent an invite to a group
    fn deny_invite(&mut self, id: GroupId) -> Result<(), Error>;

    /// Block invitations to a group
    fn block_group(&mut self, id: GroupId) -> Result<(), Error>;
}

// Group Admin Management Trait
pub trait GroupChatManagement {
    /// Create a group
    fn create_group(&mut self, name: &str) -> Result<Group, Error>;

    /// Change group name
    fn change_group_name(&mut self, id: GroupId, name: &str) -> Result<(), Error>;

    /// Open group for invites
    fn open_group(&mut self, id: GroupId) -> Result<(), Error>;

    /// Close group for invites
    fn close_group(&mut self, id: GroupId) -> Result<(), Error>;

    /// Change the administrator of the group
    fn change_admin(&mut self, id: GroupId, member: GroupMember) -> Result<(), Error>;

    /// Assign an administrator to the group
    fn assign_admin(&mut self, id: GroupId, member: GroupMember) -> Result<(), Error>;

    /// Kick member from group
    fn kick_member(&mut self, id: GroupId, member: GroupMember) -> Result<(), Error>;

    /// Ban member from group
    fn ban_member(&mut self, id: GroupId, member: GroupMember) -> Result<(), Error>;
}

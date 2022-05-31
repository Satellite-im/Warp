use crate::error::Error;
use crate::multipass::identity::PublicKey;
use crate::raygun::Uid;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub enum GroupStatus {
    Opened,
    Closed,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct GroupId(Uid);

impl GroupId {
    pub fn from_id(id: Uuid) -> GroupId {
        Self(Uid::Id(id))
    }

    pub fn from_public_key(pubkey: PublicKey) -> GroupId {
        Self(Uid::PublicKey(pubkey))
    }

    pub fn get_id(&self) -> Option<Uuid> {
        match &self.0 {
            Uid::Id(id) => Some(*id),
            Uid::PublicKey(_) => None,
        }
    }

    pub fn get_public_key(&self) -> Option<PublicKey> {
        match &self.0 {
            Uid::Id(_) => None,
            Uid::PublicKey(k) => Some(k.clone()),
        }
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct GroupMember(Uid);

impl GroupMember {
    pub fn from_id(id: Uuid) -> GroupMember {
        Self(Uid::Id(id))
    }

    pub fn from_public_key(pubkey: PublicKey) -> GroupMember {
        Self(Uid::PublicKey(pubkey))
    }

    pub fn get_id(&self) -> Option<Uuid> {
        match &self.0 {
            Uid::Id(id) => Some(*id),
            Uid::PublicKey(_) => None,
        }
    }

    pub fn get_public_key(&self) -> Option<PublicKey> {
        match &self.0 {
            Uid::Id(_) => None,
            Uid::PublicKey(k) => Some(k.clone()),
        }
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct Group {
    id: GroupId,
    name: String,
    creator: GroupMember,
    admin: GroupMember,
    members: u32,
    status: GroupStatus,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct GroupInvitation {
    id: Uuid,
    group: GroupId,
    sender: GroupMember,
    recipient: GroupMember,
}

// General/Base GroupChat Trait
pub trait GroupChat: GroupInvite + GroupChatManagement {
    fn join_group(&mut self, id: GroupId) -> Result<(), Error>;
    fn leave_group(&mut self, id: GroupId) -> Result<(), Error>;
    fn list_members(&self) -> Result<Vec<GroupMember>, Error>;
}

// Group Invite Management Trait
pub trait GroupInvite {
    fn send_invite(&mut self, id: GroupId, recipient: GroupMember) -> Result<(), Error>;
    fn accept_invite(&mut self, id: GroupId) -> Result<(), Error>;
    fn deny_invite(&mut self, id: GroupId) -> Result<(), Error>;
    fn block_group(&mut self, id: GroupId) -> Result<(), Error>;
}

// Group Admin Management Trait
pub trait GroupChatManagement {
    fn create_group(&mut self, name: &str) -> Result<Group, Error>;
    fn change_group_name(&mut self, name: &str) -> Result<(), Error>;
    fn open_group(&mut self) -> Result<(), Error>;
    fn close_group(&mut self) -> Result<(), Error>;
    fn change_admin(&mut self, member: GroupMember) -> Result<(), Error>;
    fn assign_admin(&mut self, member: GroupMember) -> Result<(), Error>;
    fn kick_member(&mut self, member: GroupMember) -> Result<(), Error>;
    fn ban_member(&mut self, member: GroupMember) -> Result<(), Error>;
}

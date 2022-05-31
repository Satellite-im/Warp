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

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct GroupInvitation {
    id: Uuid,
    group: GroupId,
    sender: GroupMember,
    recipient: GroupMember,
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
}

// General/Base GroupChat Trait
pub trait GroupChat: GroupInvite + GroupChatManagement {
    /// Join a existing group
    fn join_group(&mut self, id: GroupId) -> Result<(), Error>;

    /// Leave a group
    fn leave_group(&mut self, id: GroupId) -> Result<(), Error>;

    /// List members of group
    fn list_members(&self) -> Result<Vec<GroupMember>, Error>;
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

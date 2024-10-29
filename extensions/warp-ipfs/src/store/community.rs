use super::{topics::ConversationTopic, PeerIdExt};
use crate::store::DidExt;
use chrono::{DateTime, Utc};
use core::hash::Hash;
use indexmap::{IndexMap, IndexSet};
use rust_ipfs::Keypair;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp::{
    crypto::DID,
    error::Error,
    raygun::community::{
        Community, CommunityChannel, CommunityChannelPermission, CommunityChannelPermissions,
        CommunityChannelType, CommunityInvite, CommunityPermission, CommunityPermissions,
        CommunityRole, CommunityRoles, RoleId,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommunityRoleDocument {
    pub id: RoleId,
    pub name: String,
    pub members: IndexSet<DID>,
}
impl CommunityRoleDocument {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            members: IndexSet::new(),
        }
    }
}
impl From<CommunityRoleDocument> for CommunityRole {
    fn from(value: CommunityRoleDocument) -> Self {
        let mut role = CommunityRole::default();
        role.set_id(value.id);
        role.set_name(value.name);
        role.set_members(value.members);
        role
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommunityInviteDocument {
    pub id: Uuid,
    pub target_user: Option<DID>,
    pub created: DateTime<Utc>,
    pub expiry: Option<DateTime<Utc>>,
}
impl CommunityInviteDocument {
    pub fn new(target_user: Option<DID>, expiry: Option<DateTime<Utc>>) -> Self {
        Self {
            id: Uuid::new_v4(),
            target_user,
            created: Utc::now(),
            expiry,
        }
    }
}
impl From<CommunityInviteDocument> for CommunityInvite {
    fn from(value: CommunityInviteDocument) -> Self {
        let mut community_invite = CommunityInvite::default();
        community_invite.set_id(value.id);
        community_invite.set_target_user(value.target_user);
        community_invite.set_created(value.created);
        community_invite.set_expiry(value.expiry);
        community_invite
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq)]
pub struct CommunityDocument {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub creator: DID,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub members: IndexSet<DID>,
    pub channels: IndexMap<String, CommunityChannelDocument>,
    pub roles: IndexMap<String, CommunityRoleDocument>,
    pub permissions: CommunityPermissions,
    pub invites: IndexMap<String, CommunityInviteDocument>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl Hash for CommunityDocument {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state)
    }
}

impl PartialEq for CommunityDocument {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl CommunityDocument {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn topic(&self) -> String {
        self.id.base()
    }

    pub fn event_topic(&self) -> String {
        self.id.event_topic()
    }

    pub fn exchange_topic(&self, did: &DID) -> String {
        self.id.exchange_topic(did)
    }

    pub fn sign(&mut self, keypair: &Keypair) -> Result<(), Error> {
        let construct = warp::crypto::hash::sha256_iter(
            [
                Some(self.id().into_bytes().to_vec()),
                Some(self.creator.to_string().as_bytes().to_vec()),
            ]
            .into_iter(),
            None,
        );

        let signature = keypair.sign(&construct).expect("not RSA");
        self.signature = Some(bs58::encode(signature).into_string());

        Ok(())
    }

    pub fn verify(&self) -> Result<(), Error> {
        let creator_pk = self.creator.to_public_key()?;

        let Some(signature) = &self.signature else {
            return Err(Error::InvalidSignature);
        };

        let signature = bs58::decode(signature).into_vec()?;

        let construct = warp::crypto::hash::sha256_iter(
            [
                Some(self.id().into_bytes().to_vec()),
                Some(self.creator.to_string().as_bytes().to_vec()),
            ]
            .into_iter(),
            None,
        );

        if !creator_pk.verify(&construct, &signature) {
            return Err(Error::InvalidSignature);
        }
        Ok(())
    }
}
impl CommunityDocument {
    pub fn new(keypair: &Keypair, name: String) -> Result<Self, Error> {
        let creator = keypair.to_did()?;

        let mut permissions = CommunityPermissions::new();
        permissions.insert(CommunityPermission::EditInfo, IndexSet::new());
        permissions.insert(CommunityPermission::ManageRoles, IndexSet::new());
        permissions.insert(CommunityPermission::ManagePermissions, IndexSet::new());
        permissions.insert(CommunityPermission::ManageMembers, IndexSet::new());
        permissions.insert(CommunityPermission::ManageChannels, IndexSet::new());
        permissions.insert(CommunityPermission::ManageInvites, IndexSet::new());

        let mut members = IndexSet::new();
        members.insert(creator.clone());

        let mut document = Self {
            id: Uuid::new_v4(),
            name,
            description: None,
            creator,
            created: Utc::now(),
            modified: Utc::now(),
            members,
            channels: IndexMap::new(),
            roles: IndexMap::new(),
            permissions: permissions,
            invites: IndexMap::new(),
            deleted: false,
            signature: None,
        };
        document.sign(keypair)?;
        Ok(document)
    }
}
impl From<CommunityDocument> for Community {
    fn from(value: CommunityDocument) -> Self {
        let mut community = Community::default();
        community.set_id(value.id);
        community.set_name(value.name);
        community.set_description(value.description);
        community.set_creator(value.creator);
        community.set_created(value.created);
        community.set_modified(value.modified);
        community.set_members(value.members);
        community.set_channels(
            value
                .channels
                .iter()
                .map(|(k, _)| RoleId::parse_str(&k).expect("should be valid uuid"))
                .collect(),
        );
        community.set_roles(
            value
                .roles
                .iter()
                .map(|(k, _)| RoleId::parse_str(&k).expect("should be valid uuid"))
                .collect(),
        );
        community.set_permissions(value.permissions);
        community.set_invites(
            value
                .invites
                .iter()
                .map(|(k, _)| RoleId::parse_str(&k).expect("should be valid uuid"))
                .collect(),
        );
        community
    }
}
impl CommunityDocument {
    pub fn has_valid_invite(&self, user: &DID) -> bool {
        for (_, invite) in &self.invites {
            let is_expired = match &invite.expiry {
                Some(expiry) => expiry < &Utc::now(),
                None => false,
            };
            let is_valid_target = match &invite.target_user {
                Some(target) => user == target,
                None => true,
            };
            if !is_expired && is_valid_target {
                return true;
            }
        }
        false
    }
    pub fn has_permission(&self, user: &DID, has_permission: &CommunityPermission) -> bool {
        if &self.creator == user {
            return true;
        }
        if self.members.contains(user) {
            if let Some(authorized_roles) = self.permissions.get(has_permission) {
                for authorized_role in authorized_roles {
                    if let Some(role) = self.roles.get(&authorized_role.to_string()) {
                        if role.members.contains(user) {
                            return true;
                        }
                    }
                }
            } else {
                return true;
            }
        }
        false
    }

    pub fn has_channel_permission(
        &self,
        user: &DID,
        has_permission: &CommunityChannelPermission,
        channel_id: Uuid,
    ) -> bool {
        if &self.creator == user {
            return true;
        }
        if self.members.contains(user) {
            if let Some(channel) = self.channels.get(&channel_id.to_string()) {
                if let Some(authorized_roles) = channel.permissions.get(has_permission) {
                    for authorized_role in authorized_roles {
                        if let Some(role) = self.roles.get(&authorized_role.to_string()) {
                            if role.members.contains(user) {
                                return true;
                            }
                        }
                    }
                } else {
                    return true;
                }
            }
        }
        false
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommunityChannelDocument {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub channel_type: CommunityChannelType,
    pub permissions: CommunityChannelPermissions,
}
impl CommunityChannelDocument {
    pub fn new(
        name: String,
        description: Option<String>,
        channel_type: CommunityChannelType,
    ) -> Self {
        let mut permissions = CommunityChannelPermissions::new();
        permissions.insert(CommunityChannelPermission::EditInfo, IndexSet::new());
        permissions.insert(CommunityChannelPermission::DeleteMessages, IndexSet::new());
        Self {
            id: Uuid::new_v4(),
            name,
            description,
            created: Utc::now(),
            modified: Utc::now(),
            channel_type,
            permissions: permissions,
        }
    }
}
impl From<CommunityChannelDocument> for CommunityChannel {
    fn from(value: CommunityChannelDocument) -> Self {
        let mut community_channel = CommunityChannel::default();
        community_channel.set_id(value.id);
        community_channel.set_name(value.name);
        community_channel.set_description(value.description);
        community_channel.set_created(value.created);
        community_channel.set_modified(value.modified);
        community_channel.set_channel_type(value.channel_type);
        community_channel.set_permissions(value.permissions);
        community_channel
    }
}

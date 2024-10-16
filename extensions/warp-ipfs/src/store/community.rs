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
        Community, CommunityChannel, CommunityChannelPermissions, CommunityChannelType, CommunityInvite, CommunityPermissions, CommunityRoles, RoleId
    },
};

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
    pub channels: IndexMap<Uuid, CommunityChannelDocument>,
    pub roles: CommunityRoles,
    pub permissions: CommunityPermissions,
    pub invites: IndexMap<Uuid, CommunityInviteDocument>,
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

    pub fn name(&self) -> &String {
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
        let did = keypair.to_did()?;
        let mut document = Self {
            id: Uuid::new_v4(),
            name,
            description: None,
            creator: did,
            created: Utc::now(),
            modified: Utc::now(),
            members: IndexSet::new(),
            channels: IndexMap::new(),
            roles: CommunityRoles::new(),
            permissions: CommunityPermissions::new(),
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
        community.set_channels(value.channels.iter().map(|(k, _)| *k).collect());
        community.set_roles(value.roles);
        community.set_permissions(value.permissions);
        community.set_invites(value.invites.iter().map(|(k, _)| *k).collect());
        community
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
        Self {
            id: Uuid::new_v4(),
            name,
            description,
            created: Utc::now(),
            modified: Utc::now(),
            channel_type,
            permissions: CommunityChannelPermissions::new(),
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

use crate::error::Error;
use crate::{
    crypto::DID,
    js_exports::stream::AsyncIterator,
    multipass::{
        self,
        identity::{self, Identity, IdentityProfile},
        MultiPass,
    },
    tesseract::Tesseract,
};
use futures::StreamExt;
use js_sys::Uint8Array;
use std::str::FromStr;
use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen]
pub struct MultiPassBox {
    inner: Box<dyn MultiPass>,
}
impl MultiPassBox {
    pub fn new(multipass: Box<dyn MultiPass>) -> Self {
        Self { inner: multipass }
    }
}

/// impl MultiPass trait
#[wasm_bindgen]
impl MultiPassBox {
    pub async fn create_identity(
        &mut self,
        username: Option<String>,
        passphrase: Option<String>,
    ) -> Result<IdentityProfile, JsError> {
        self.inner
            .create_identity(username.as_deref(), passphrase.as_deref())
            .await
            .map_err(|e| e.into())
    }

    pub async fn get_identity(
        &self,
        id_variant: Identifier,
        id_value: JsValue,
    ) -> Result<JsValue, JsError> {
        let id = to_identifier_enum(id_variant, id_value)?;
        let single_id = matches!(id, crate::multipass::identity::Identifier::DID(_));
        let list = self
            .inner
            .get_identity(id.clone())
            .collect::<Vec<_>>()
            .await;
        match (single_id, list.is_empty()) {
            (true, true) => Err(Error::IdentityDoesntExist.into()),
            (_, false) | (_, true) => Ok(serde_wasm_bindgen::to_value(&list).unwrap()),
        }
    }

    pub async fn identity(&self) -> Result<Identity, JsError> {
        self.inner.identity().await.map_err(|e| e.into())
    }

    pub fn tesseract(&self) -> Tesseract {
        self.inner.tesseract()
    }

    pub async fn update_identity(
        &mut self,
        option: IdentityUpdate,
        value: JsValue,
    ) -> Result<(), JsError> {
        self.inner
            .update_identity(to_identity_update_enum(option, value)?)
            .await
            .map_err(|e| e.into())
    }
}

/// impl MultiPassEvent trait
#[wasm_bindgen]
impl MultiPassBox {
    pub async fn multipass_subscribe(&mut self) -> Result<AsyncIterator, JsError> {
        self.inner
            .multipass_subscribe()
            .await
            .map_err(|e| e.into())
            .map(|s| {
                AsyncIterator::new(Box::pin(
                    s.map(|t| Into::<MultiPassEventKind>::into(t).into()),
                ))
            })
    }
}

/// impl Friends trait
#[wasm_bindgen]
impl MultiPassBox {
    /// Send friend request to corresponding public key
    pub async fn send_request(&mut self, pubkey: String) -> Result<(), JsError> {
        self.inner
            .send_request(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// Accept friend request from public key
    pub async fn accept_request(&mut self, pubkey: String) -> Result<(), JsError> {
        self.inner
            .accept_request(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// Deny friend request from public key
    pub async fn deny_request(&mut self, pubkey: String) -> Result<(), JsError> {
        self.inner
            .deny_request(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// Closing or retracting friend request
    pub async fn close_request(&mut self, pubkey: String) -> Result<(), JsError> {
        self.inner
            .close_request(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// Check to determine if a request been received from the DID
    pub async fn received_friend_request_from(&self, pubkey: String) -> Result<bool, JsError> {
        self.inner
            .received_friend_request_from(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// List the incoming friend request
    pub async fn list_incoming_request(&self) -> Result<JsValue, JsError> {
        self.inner
            .list_incoming_request()
            .await
            .map_err(|e| e.into())
            .map(|ok| {
                serde_wasm_bindgen::to_value(
                    &ok.iter().map(|i| i.to_string()).collect::<Vec<String>>(),
                )
                .unwrap()
            })
    }

    /// Check to determine if a request been sent to the DID
    pub async fn sent_friend_request_to(&self, pubkey: String) -> Result<bool, JsError> {
        self.inner
            .sent_friend_request_to(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// List the outgoing friend request
    pub async fn list_outgoing_request(&self) -> Result<JsValue, JsError> {
        self.inner
            .list_outgoing_request()
            .await
            .map_err(|e| e.into())
            .map(|ok| {
                serde_wasm_bindgen::to_value(
                    &ok.iter().map(|i| i.to_string()).collect::<Vec<String>>(),
                )
                .unwrap()
            })
    }

    /// Remove friend from contacts
    pub async fn remove_friend(&mut self, pubkey: String) -> Result<(), JsError> {
        self.inner
            .remove_friend(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// Block public key, rather it be a friend or not, from being able to send request to account public address
    pub async fn block(&mut self, pubkey: String) -> Result<(), JsError> {
        self.inner
            .block(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// Unblock public key
    pub async fn unblock(&mut self, pubkey: String) -> Result<(), JsError> {
        self.inner
            .unblock(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// List block list
    pub async fn block_list(&self) -> Result<JsValue, JsError> {
        self.inner
            .block_list()
            .await
            .map_err(|e| e.into())
            .map(|ok| {
                serde_wasm_bindgen::to_value(
                    &ok.iter().map(|i| i.to_string()).collect::<Vec<String>>(),
                )
                .unwrap()
            })
    }

    /// Check to see if public key is blocked
    pub async fn is_blocked(&self, pubkey: String) -> Result<bool, JsError> {
        self.inner
            .is_blocked(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }

    /// List all friends public key
    pub async fn list_friends(&self) -> Result<JsValue, JsError> {
        self.inner
            .list_friends()
            .await
            .map_err(|e| e.into())
            .map(|ok| {
                serde_wasm_bindgen::to_value(
                    &ok.iter().map(|i| i.to_string()).collect::<Vec<String>>(),
                )
                .unwrap()
            })
    }

    /// Check to see if public key is friend of the account
    pub async fn has_friend(&self, pubkey: String) -> Result<bool, JsError> {
        self.inner
            .has_friend(&DID::from_str(&pubkey).unwrap_or_default())
            .await
            .map_err(|e| e.into())
    }
}

#[wasm_bindgen]
pub enum IdentityUpdate {
    Username,
    Picture,
    PicturePath,
    PictureStream,
    ClearPicture,
    Banner,
    BannerPath,
    BannerStream,
    ClearBanner,
    StatusMessage,
    ClearStatusMessage,
}

fn to_identity_update_enum(
    option: IdentityUpdate,
    value: JsValue,
) -> Result<identity::IdentityUpdate, JsError> {
    match option {
        IdentityUpdate::Username => match value.as_string() {
            Some(s) => Ok(identity::IdentityUpdate::Username(s)),
            None => Err(JsError::new("JsValue is not a string")),
        },
        IdentityUpdate::Picture => Ok(identity::IdentityUpdate::Picture(
            Uint8Array::new(&value).to_vec(),
        )),
        IdentityUpdate::PicturePath => Ok(identity::IdentityUpdate::PicturePath(
            value
                .as_string()
                .ok_or(JsError::new("JsValue is not a string"))?
                .into(),
        )),
        // IdentityUpdate::PictureStream => Ok(identity::IdentityUpdate::PictureStream(
        //     value.into()
        // )),
        IdentityUpdate::ClearPicture => Ok(identity::IdentityUpdate::ClearPicture),
        IdentityUpdate::Banner => Ok(identity::IdentityUpdate::Banner(
            Uint8Array::new(&value).to_vec(),
        )),
        IdentityUpdate::BannerPath => Ok(identity::IdentityUpdate::BannerPath(
            value
                .as_string()
                .ok_or(JsError::new("JsValue is not a string"))?
                .into(),
        )),
        // IdentityUpdate::BannerStream => Ok(identity::IdentityUpdate::BannerStream(
        //     value.into()
        // )),
        IdentityUpdate::ClearBanner => Ok(identity::IdentityUpdate::ClearBanner),
        IdentityUpdate::StatusMessage => {
            if value.is_null() {
                return Ok(identity::IdentityUpdate::StatusMessage(None));
            }
            match value.as_string() {
                Some(s) => Ok(identity::IdentityUpdate::StatusMessage(Some(s))),
                None => Err(JsError::new("JsValue is not a string")),
            }
        }
        IdentityUpdate::ClearStatusMessage => Ok(identity::IdentityUpdate::ClearStatusMessage),
        _ => Err(JsError::new("IdentityUpdate variant not yet implemented")),
    }
}

#[wasm_bindgen]
pub enum Identifier {
    DID,
    DIDList,
    Username,
}
fn to_identifier_enum(option: Identifier, value: JsValue) -> Result<identity::Identifier, JsError> {
    match option {
        Identifier::DID => match value.as_string() {
            Some(did) => Ok(identity::Identifier::DID(DID::from_str(did.as_str())?)),
            None => Err(JsError::new("JsValue is not a string")),
        },
        Identifier::DIDList => {
            let iterator = match js_sys::try_iter(&value) {
                Err(e) => Err(JsError::new(e.as_string().unwrap_or_default().as_str())),
                Ok(value) => match value {
                    None => Err(JsError::new("JsValue is not iterable")),
                    Some(value) => Ok(value),
                },
            }?;

            let mut did_list = Vec::<DID>::new();
            for item in iterator {
                let item = match item {
                    Err(e) => Err(JsError::new(e.as_string().unwrap_or_default().as_str())),
                    Ok(value) => Ok(value),
                }?;
                let str = item
                    .as_string()
                    .ok_or_else(|| JsError::new("JsValue is not a string"))?;
                did_list.push(DID::from_str(&str)?);
            }
            Ok(identity::Identifier::DIDList(did_list))
        }
        Identifier::Username => match value.as_string() {
            Some(s) => Ok(identity::Identifier::Username(s)),
            None => Err(JsError::new("JsValue is not a string")),
        },
    }
}

impl From<multipass::MultiPassEventKind> for MultiPassEventKind {
    fn from(value: multipass::MultiPassEventKind) -> Self {
        match value {
            multipass::MultiPassEventKind::FriendRequestReceived { from } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::FriendRequestReceived,
                did: from.to_string(),
            },
            multipass::MultiPassEventKind::FriendRequestSent { to } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::FriendRequestSent,
                did: to.to_string(),
            },
            multipass::MultiPassEventKind::IncomingFriendRequestRejected { did } => {
                MultiPassEventKind {
                    kind: MultiPassEventKindEnum::IncomingFriendRequestRejected,
                    did: did.to_string(),
                }
            }
            multipass::MultiPassEventKind::OutgoingFriendRequestRejected { did } => {
                MultiPassEventKind {
                    kind: MultiPassEventKindEnum::OutgoingFriendRequestRejected,
                    did: did.to_string(),
                }
            }
            multipass::MultiPassEventKind::IncomingFriendRequestClosed { did } => {
                MultiPassEventKind {
                    kind: MultiPassEventKindEnum::IncomingFriendRequestClosed,
                    did: did.to_string(),
                }
            }
            multipass::MultiPassEventKind::OutgoingFriendRequestClosed { did } => {
                MultiPassEventKind {
                    kind: MultiPassEventKindEnum::OutgoingFriendRequestClosed,
                    did: did.to_string(),
                }
            }
            multipass::MultiPassEventKind::FriendAdded { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::FriendAdded,
                did: did.to_string(),
            },
            multipass::MultiPassEventKind::FriendRemoved { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::FriendRemoved,
                did: did.to_string(),
            },
            multipass::MultiPassEventKind::IdentityOnline { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::IdentityOnline,
                did: did.to_string(),
            },
            multipass::MultiPassEventKind::IdentityOffline { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::IdentityOffline,
                did: did.to_string(),
            },
            multipass::MultiPassEventKind::IdentityUpdate { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::IdentityUpdate,
                did: did.to_string(),
            },
            multipass::MultiPassEventKind::Blocked { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::Blocked,
                did: did.to_string(),
            },
            multipass::MultiPassEventKind::BlockedBy { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::BlockedBy,
                did: did.to_string(),
            },
            multipass::MultiPassEventKind::Unblocked { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::Unblocked,
                did: did.to_string(),
            },
            multipass::MultiPassEventKind::UnblockedBy { did } => MultiPassEventKind {
                kind: MultiPassEventKindEnum::UnblockedBy,
                did: did.to_string(),
            },
        }
    }
}
#[wasm_bindgen]
pub struct MultiPassEventKind {
    kind: MultiPassEventKindEnum,
    did: String,
}

#[wasm_bindgen]
impl MultiPassEventKind {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> MultiPassEventKindEnum {
        self.kind
    }
    #[wasm_bindgen(getter)]
    pub fn did(&self) -> String {
        self.did.clone()
    }
}

#[derive(Copy, Clone)]
#[wasm_bindgen]
pub enum MultiPassEventKindEnum {
    FriendRequestReceived,
    FriendRequestSent,
    IncomingFriendRequestRejected,
    OutgoingFriendRequestRejected,
    IncomingFriendRequestClosed,
    OutgoingFriendRequestClosed,
    FriendAdded,
    FriendRemoved,
    IdentityOnline,
    IdentityOffline,
    IdentityUpdate,
    Blocked,
    BlockedBy,
    Unblocked,
    UnblockedBy,
}

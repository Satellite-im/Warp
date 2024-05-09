use crate::crypto::DID;
use crate::multipass::{
    identity::{self, Identity, IdentityProfile},
    MultiPass,
};
use js_sys::Uint8Array;
use std::str::FromStr;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WarpInstance {
    multipass: MultiPassBox,
    //pub raygun: RayGunBox,
    //pub constellation: ConstellationBox
}

impl WarpInstance {
    pub fn new(
        mp: Box<dyn MultiPass>, /*rg: Box<dyn RayGun>, fs: Box<dyn Constellation>*/
    ) -> Self {
        let multipass = MultiPassBox::new(mp);
        //let raygun = RayGunBox::new(rg);
        //let constellation = ConstellationBox::new(fs);

        Self {
            multipass,
            //raygun,
            //constellation
        }
    }
}
#[wasm_bindgen]
impl WarpInstance {
    #[wasm_bindgen(getter)]
    pub fn multipass(&self) -> MultiPassBox {
        self.multipass.clone()
    }
}

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
        self.inner
            .get_identity(to_identifier_enum(id_variant, id_value)?)
            .await
            .map_err(|e| e.into())
            .map(|ok| serde_wasm_bindgen::to_value(&ok).unwrap())
    }

    pub async fn get_own_identity(&self) -> Result<Identity, JsError> {
        self.inner.get_own_identity().await.map_err(|e| e.into())
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
    Own,
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
        Identifier::Own => Ok(identity::Identifier::Own),
    }
}

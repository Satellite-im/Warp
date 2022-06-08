/// Errors that would host custom errors for modules, utilities, etc.
use thiserror::Error;
use wasm_bindgen::JsValue;

#[derive(Error, Debug)]
pub enum Error {
    //Hook Errors
    #[error("Hook is not registered")]
    HookUnregistered,
    #[error("Hook with this name already registered")]
    DuplicateHook,
    #[error("Already subscribed to this hook")]
    AlreadySubscribed,

    //Constellation Errors
    #[error("Constellation extension is unavailable")]
    ConstellationExtensionUnavailable,
    #[error("Item with name already exists in current directory")]
    DuplicateName,
    #[error("Directory cannot contain itself")]
    DirParadox,
    #[error("Directory cannot contain one of its ancestors")]
    DirParentParadox,
    #[error("Directory cannot be found or is invalid")]
    DirInvalid,
    #[error("File cannot be found or is invalid")]
    FileInvalid,
    #[error("Item cannot be found or is invalid")]
    ItemInvalid,
    #[error("Item is not a valid file")]
    ItemNotFile,
    #[error("Item is not a valid Directory")]
    ItemNotDirectory,
    #[error("Attempted conversion is invalid")]
    InvalidConversion,
    #[error("Path supplied is invalid")]
    InvalidPath,

    //PocketDimension Errors
    #[error("Pocket dimension extension is unavailable")]
    PocketDimensionExtensionUnavailable,
    #[error("Data module supplied does not match dimension module")]
    DimensionMismatch,
    #[error("Data object provided already exist within the dimension")]
    DataObjectExist,
    #[error("Data object does not exist within the dimension")]
    DataObjectNotFound,

    //MultiPass Errors
    #[error("MultiPass extension is unavailable")]
    MultiPassExtensionUnavailable,
    #[error("Username has to be atleast 3 characters long")]
    UsernameTooShort,
    #[error("Username cannot be more than 32 characters long")]
    UsernameTooLong,
    #[error("Identity exist with the same information")]
    IdentityExist,
    #[error("Identity does not exist")]
    IdentityDoesntExist,
    #[error("Username cannot be updated for identity")]
    CannotUpdateIdentityUsername,
    #[error("Picture cannot be updated for identity")]
    CannotUpdateIdentityPicture,
    #[error("Banner cannot be updated for identity")]
    CannotUpdateIdentityBanner,
    #[error("Status cannot be updated for identity")]
    CannotUpdateIdentityStatus,
    #[error("Identity could not be updated")]
    CannotUpdateIdentity,
    #[error("Unable to send a friend request")]
    CannotSendFriendRequest,
    #[error("You cannot send yourself a friend request")]
    CannotSendSelfFriendRequest,
    #[error("You cannot accept yourself as a friend")]
    CannotAcceptSelfAsFriend,
    #[error("You cannot deny yourself as a friend")]
    CannotDenySelfAsFriend,
    #[error("You cannot block yourself as a friend")]
    CannotBlockSelfAsFriend,
    #[error("You cannot remove yourself as a friend")]
    CannotRemoveSelfAsFriend,
    #[error("You cannot use yourself")]
    CannotUseSelfAsFriend,
    #[error("Unable to close friend request")]
    CannotCloseFriendRequest,
    #[error("User does not exist as a friend")]
    FriendDoesntExist,
    #[error("User already exist as a friend")]
    FriendExist,
    #[error("User has blocked you from being able to interact with them")]
    BlockedByUser,
    #[error("Invalid identifier condition provided. Must be either public key, username, or your own identity")]
    InvalidIdentifierCondition,

    //RayGun Errors
    #[error("RayGun extension is unavailable")]
    RayGunExtensionUnavailable,
    #[error("Conversation was invalid")]
    InvalidConversation,
    #[error("Message is empty")]
    EmptyMessage,
    #[error("Message was invalid")]
    InvalidMessage,
    #[error("Sender of the message is invalid")]
    SenderMismatch,
    #[error("Cannot react to the message with the same reaction")]
    ReactionExist,
    #[error("Reaction to the message does not exist")]
    ReactionDoesntExist,
    #[error("Message is already pinned")]
    MessagePinned,
    #[error("Message is not pinned")]
    MessageNotPinned,
    #[error("Group could not be created at this time")]
    CannotCreateGroup,
    #[error("Unable to join group")]
    CannotJoinGroup,
    #[error("Unable to obtain the list of members from the group")]
    CannotGetMembers,
    #[error("Invalid Group Id")]
    InvalidGroupId,
    #[error("Invalid Group Member")]
    InvalidGroupMemeber,
    #[error("Invite is invalid")]
    InvalidInvite,
    #[error("Unable to change group status")]
    CannotChangeGroupStatus,
    #[error("Group name exceed maximum limit")]
    GroupNameTooLong,
    #[error("Group name does not meet minimum limit")]
    GroupNameTooShort,
    #[error("Group is closed")]
    GroupClosed,
    #[error("Group is opened")]
    GroupOpened,

    //Crypto Errors
    #[error("{0}")]
    Ed25519Error(#[from] ed25519_dalek::SignatureError),
    #[error("Unable to encrypt data")]
    EncryptionError,
    #[error("Unable to decrypt data")]
    DecryptionError,
    #[error("Unable to encrypt stream")]
    EncryptionStreamError,
    #[error("Unable to decrypt stream")]
    DecryptionStreamError,
    #[error("Public key is invalid")]
    PublicKeyInvalid,
    #[error("Private key is invalid")]
    PrivateKeyInvalid,
    #[error("Public key length is invalid")]
    InvalidPublicKeyLength,
    #[error("Private key length is invalid")]
    InvalidPrivateKeyLength,

    //Tesseract Errors
    #[error("Tesseract is unavailable")]
    TesseractUnavailable,
    #[error("Tesseract is locked")]
    TesseractLocked,
    #[error("One or more items in the datastore are corrupted or invalid")]
    CorruptedDataStore,
    #[error("Unable to save tesseract")]
    CannotSaveTesseract,

    //Data Errors
    #[error("Invalid data type")]
    InvalidDataType,

    //Misc
    #[error("Async runtime is unavailable")]
    AsyncRuntimeUnavailable,
    #[error("Sender Channel Unavailable")]
    SenderChannelUnavailable,
    #[error("Receiver Channel Unavailable")]
    ReceiverChannelUnavailable,
    #[error("Cannot find position of array content.")]
    ArrayPositionNotFound,
    #[error("Object is not found")]
    ObjectNotFound,
    #[error("The length of the key is invalid")]
    InvalidKeyLength,
    #[error("File is not found")]
    FileNotFound,
    #[error("Directory is not found")]
    DirectoryNotFound,
    #[error("Error to be determined")]
    ToBeDetermined,
    #[error("{0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("{0}")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("Cannot deserialize: {0}")]
    TomlDeserializeError(#[from] toml::de::Error),
    #[error("Cannot serialize: {0}")]
    TomlSerializeError(#[from] toml::ser::Error),
    #[error("{0}")]
    RegexError(#[from] regex::Error),
    #[error(transparent)]
    Any(#[from] anyhow::Error),
    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("Functionality is not yet implemented")]
    Unimplemented,
    #[error("An unknown error has occurred")]
    Other,
}

impl Error {
    pub fn enum_to_string(&self) -> String {
        match &self {
            Error::HookUnregistered => String::from("HookUnregistered"),
            Error::DuplicateHook => String::from("DuplicateHook"),
            Error::AlreadySubscribed => String::from("AlreadySubscribed"),
            Error::DuplicateName => String::from("DuplicateName"),
            Error::DirParadox => String::from("DirParadox"),
            Error::DirParentParadox => String::from("DirParentParadox"),
            Error::DirInvalid => String::from("DirInvalid"),
            Error::ItemInvalid => String::from("ItemInvalid"),
            Error::ItemNotFile => String::from("ItemNotFile"),
            Error::ItemNotDirectory => String::from("ItemNotDirectory"),
            Error::InvalidConversion => String::from("InvalidConversion"),
            Error::InvalidPath => String::from("InvalidPath"),
            Error::ArrayPositionNotFound => String::from("ArrayPositionNotFound"),
            Error::DimensionMismatch => String::from("DimensionMismatch"),
            Error::DataObjectExist => String::from("DataObjectExist"),
            Error::DataObjectNotFound => String::from("DataObjectNotFound"),
            Error::Ed25519Error(_) => String::from("Ed25519Error"),
            Error::InvalidDataType => String::from("InvalidDataType"),
            Error::ObjectNotFound => String::from("ObjectNotFound"),
            Error::InvalidKeyLength => String::from("InvalidKeyLength"),
            Error::FileNotFound => String::from("FileNotFound"),
            Error::DirectoryNotFound => String::from("DirectoryNotFound"),
            Error::ToBeDetermined => String::from("ToBeDetermined"),
            Error::EncryptionError => String::from("EncryptionError"),
            Error::DecryptionError => String::from("DecryptionError"),
            Error::EncryptionStreamError => String::from("EncryptionStreamError"),
            Error::DecryptionStreamError => String::from("DecryptionStreamError"),
            Error::TesseractLocked => String::from("TesseractLocked"),
            Error::SerdeJsonError(_) => String::from("SerdeJsonError"),
            Error::SerdeYamlError(_) => String::from("SerdeYamlError"),
            Error::TomlDeserializeError(_) => String::from("TomlDeserializeError"),
            Error::TomlSerializeError(_) => String::from("TomlSerializeError"),
            Error::RegexError(_) => String::from("RegexError"),
            Error::Any(_) => String::from("Any"),
            Error::IoError(_) => String::from("IoError"),
            Error::Unimplemented => String::from("Unimplemented"),
            Error::Other | _ => String::from("Other"),
        }
    }
}

impl From<Error> for JsValue {
    fn from(error: Error) -> JsValue {
        JsValue::from_str(&error.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
pub fn into_error(error: Error) -> wasm_bindgen::JsError {
    wasm_bindgen::JsError::from(error)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn into_error(error: Error) -> Error {
    error
}

/// Errors that would host custom errors for modules, utilities, etc.
use thiserror::Error;

#[derive(Error, Debug)]
#[cfg(not(feature = "bincode_opt"))]
pub enum Error {
    //Hook Errors
    #[error("Hook is not registered")]
    HookUnregistered,
    #[error("Hook with this name already registered")]
    DuplicateHook,
    #[error("Already subscribed to this hook")]
    AlreadySubscribed,

    //Constellation Errors
    #[error("Item with name already exists in current directory")]
    DuplicateName,
    #[error("Directory cannot contain itself")]
    DirParadox,
    #[error("Directory cannot contain one of its ancestors")]
    DirParentParadox,
    #[error("Directory cannot be found or is invalid")]
    DirInvalid,
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
    #[error("Cannot find position of array content.")]
    ArrayPositionNotFound,

    //PocketDimension Errors
    #[error("Data module supplied does not match dimension module")]
    DimensionMismatch,
    #[error("Data object provided already exist within the dimension")]
    DataObjectExist,

    //Misc
    #[error("TBD")]
    FileNotFound,
    #[error("TBD")]
    DirectoryNotFound,
    #[error("To be determined")]
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

//NOTE: This is temporary to resolve building error while looking into related errors
//TODO: Research `cfg_attr` and if its allowed to be used within an enum.
#[derive(Error, Debug)]
#[cfg(feature = "bincode_opt")]
pub enum Error {
    //Hook Errors
    #[error("Hook is not registered")]
    HookUnregistered,
    #[error("Hook with this name already registered")]
    DuplicateHook,
    #[error("Already subscribed to this hook")]
    AlreadySubscribed,

    //Constellation Errors
    #[error("Item with name already exists in current directory")]
    DuplicateName,
    #[error("Directory cannot contain itself")]
    DirParadox,
    #[error("Directory cannot contain one of its ancestors")]
    DirParentParadox,
    #[error("Directory cannot be found or is invalid")]
    DirInvalid,
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
    #[error("Cannot find position of array content.")]
    ArrayPositionNotFound,

    //PocketDimension Errors
    #[error("Data module supplied does not match dimension module")]
    DimensionMismatch,
    #[error("Data object provided already exist within the dimension")]
    DataObjectExist,

    //Misc
    #[error("TBD")]
    FileNotFound,
    #[error("TBD")]
    DirectoryNotFound,
    #[error("TBD")]
    ToBeDetermined,
    #[error("{0}")]
    BincodeError(#[from] bincode::Error),
    #[error("{0}")]
    SerdeJsonError(#[from] serde_json::Error),
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

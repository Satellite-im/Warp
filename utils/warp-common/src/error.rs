/// Errors that would host custom errors for modules, utilities, etc.
use thiserror::Error;

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
    #[error("Data object does not exist within tje dimension")]
    DataObjectNotFound,

    //Misc
    #[error("Invalid data type")]
    InvalidDataType,
    #[error("Object is not found")]
    ObjectNotFound,
    #[error("The length of the key is invalid")]
    InvalidKeyLength,
    #[error("File is not found")]
    FileNotFound,
    #[error("Directory is not found")]
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
    OtherError(#[from] OptionalError),
    #[error(transparent)]
    Any(#[from] anyhow::Error),
    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("Functionality is not yet implemented")]
    Unimplemented,
    #[error("An unknown error has occurred")]
    Other,
}

// Since `bincode` is a optional crate, we cannot easily include it in the regular
// Error enum without it giving an error, so we are pushing it into its own enum to handle bincode error
// (and possibly other optional errors in the future) with a condition that if bincode feature flag isnt used to
// supply an empty enum.
cfg_if::cfg_if! {
    if #[cfg(feature = "bincode_opt")] {
        #[derive(Error, Debug)]
        pub enum OptionalError {
            #[error("{0}")]
            BincodeError(#[from] bincode::Error),
        }
    } else {
        #[derive(Error, Debug)]
        pub enum OptionalError {
        }
    }
}

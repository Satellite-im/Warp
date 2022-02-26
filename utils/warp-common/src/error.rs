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
    //TODO

    //Misc
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
    #[error("An unknown error has occurred")]
    Other,
}

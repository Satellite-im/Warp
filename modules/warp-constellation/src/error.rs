use thiserror::Error;

/// Errors
/// 


#[derive(Error, Debug, PartialEq)]
pub enum Error {
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
    #[error("Cannot find position of array content.")]
    ArrayPositionNotFound,
    #[error("Regex Error: {0}")]
    RegexError(#[from] regex::Error),
    #[error("Unknown error has occurred")]
    Other
}
use thiserror::Error;

/// Errors
/// 


#[derive(Error, Debug, PartialEq)]
pub enum Error {
    #[error("Method not implemented.")]
    MethodMissing,
    #[error("RFM class is Abstract. It can only be extended")]
    RfmAbstractOnly,
    #[error("Item class is Abstract. It can only be extended")]
    ItemAbstractOnly,
    #[error("Item name must be a non empty string")]
    NoEmptyString,
    #[error("Item name contains invalid symbol")]
    InvalidSymbol,
    #[error("Item with name already exists in this directory")]
    DuplicateName,
    #[error("Directory cannot contain itself")]
    DirParadox,
    #[error("Directory cannot contain one of its ancestors")]
    DirParentParadox,
    #[error("Directory cannot be found or is invalid")]
    DirInvalid,
    #[error("Item cannot be found or is invalid")]
    ItemInvalid,
    #[error("Attempted conversion is invalid")]
    InvalidConversion,
    #[error("Cannot find position of array content.")]
    ArrayPositionNotFound,
    #[error("Regex Error: {0}")]
    RegexError(#[from] regex::Error),
    #[error("Unknown error has occurred")]
    Other
}
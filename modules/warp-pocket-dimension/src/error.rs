use thiserror::Error;



#[derive(Error, Debug)]
pub enum Error {
    #[error("Regex Error: {0}")]
    RegexError(#[from] regex::Error),
    #[error("{0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("{0}")]
    WarpDataError(#[from] warp_data::error::Error),
    #[error("Unknown error has occurred")]
    Other
}
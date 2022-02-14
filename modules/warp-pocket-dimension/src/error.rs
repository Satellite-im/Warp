use thiserror::Error;



#[derive(Error, Debug, PartialEq)]
pub enum Error {
    #[error("Regex Error: {0}")]
    RegexError(#[from] regex::Error),
    #[error("Unknown error has occurred")]
    Other
}
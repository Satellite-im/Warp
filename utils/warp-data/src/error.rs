use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("Unknown error has occurred")]
    Other,
}

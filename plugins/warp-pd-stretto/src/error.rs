use thiserror::Error;
use warp_common::serde_json;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("{0}")]
    WarpError(#[from] warp_common::error::Error),
    #[error("{0}")]
    StrettoError(#[from] stretto::CacheError),
}

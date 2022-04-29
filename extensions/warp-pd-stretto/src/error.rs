use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    SerdeJsonError(#[from] serde_json::Error),
    #[error("{0}")]
    WarpError(#[from] warp::error::Error),
    #[error("{0}")]
    StrettoError(#[from] stretto::CacheError),
}

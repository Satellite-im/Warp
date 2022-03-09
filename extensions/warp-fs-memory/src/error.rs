use thiserror::Error;
use warp_common::anyhow;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("Unknown or Other Error")]
    Other,
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

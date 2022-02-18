use thiserror::Error;

/// Errors

#[derive(Error, Debug, PartialEq)]
pub enum Error {
    #[error("Hook is not registered")]
    HookUnregistered,
    #[error("Hook with this name already registered")]
    DuplicateHook,
    #[error("Already subscribed to this hook")]
    AlreadySubscribed,
    #[error("Unknown error has occurred")]
    Other,
}

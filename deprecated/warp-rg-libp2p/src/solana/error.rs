use thiserror::Error;

#[derive(Error, Debug)]
pub enum GroupError {
    #[error("User cannot perform this action")]
    WrongPrivileges,
    #[error("Invite does not match Group ID")]
    InvitationMismatch,
    #[error("Account was not created by provided user")]
    PayerMismatch,
    #[error("Group not empty")]
    NotEmpty,
    #[error("The field is too short or too long")]
    IncorrectField,
    #[error("Parameters order mismatch")]
    InputError,
    #[error("Unknown Error Code: {0}")]
    Unknown(u32),
}

impl From<u32> for GroupError {
    fn from(code: u32) -> Self {
        match code {
            6000 => GroupError::WrongPrivileges,
            6001 => GroupError::InvitationMismatch,
            6002 => GroupError::PayerMismatch,
            6003 => GroupError::NotEmpty,
            6004 => GroupError::IncorrectField,
            6005 => GroupError::InputError,
            code => GroupError::Unknown(code),
        }
    }
}

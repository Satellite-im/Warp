use thiserror::Error;

#[derive(Error, Debug)]
pub enum FriendsError {
    #[error("Addresses in request don't match user address")]
    WrongRequestData,
    #[error("Request is not pending")]
    NotPendingRequest,
    #[error("Accounts are not friends yet")]
    NotFriends,
    #[error("User can't perform this action")]
    WrongPrivileges,
    #[error("User1 and user2 needs to be passed in order")]
    OrderMismatch,
    #[error("Users are already friends")]
    AlreadyFriends,
    #[error("Request already existent")]
    ExistentRequest,
    #[error("Account was not created by provided user")]
    PayerMismatch,
    #[error("Request is not removed yet")]
    NotRemoved,
    #[error("Request is already removed")]
    AlreadyRemoved,
    #[error("Unknown Error Code: {0}")]
    Unknown(u32),
}

impl From<u32> for FriendsError {
    fn from(code: u32) -> Self {
        match code {
            6000 => FriendsError::WrongRequestData,
            6001 => FriendsError::NotPendingRequest,
            6002 => FriendsError::NotFriends,
            6003 => FriendsError::WrongPrivileges,
            6004 => FriendsError::OrderMismatch,
            6005 => FriendsError::AlreadyFriends,
            6006 => FriendsError::ExistentRequest,
            6007 => FriendsError::PayerMismatch,
            6008 => FriendsError::NotRemoved,
            6009 => FriendsError::AlreadyRemoved,
            code => FriendsError::Unknown(code),
        }
    }
}

#[derive(Error, Debug)]
pub enum UserError {
    #[error("User cannot perform this action")]
    WrongPrivileges,
    #[error("Account was not created by provided user")]
    PayerMismatch,
    #[error("The field is too short or too long")]
    IncorrectField,
    #[error("Parameters order mismatch")]
    InputError,
    #[error("Unknown Error Code: {0}")]
    Unknown(u32),
}

impl From<u32> for UserError {
    fn from(code: u32) -> Self {
        match code {
            6000 => UserError::WrongPrivileges,
            6001 => UserError::PayerMismatch,
            6002 => UserError::IncorrectField,
            6003 => UserError::InputError,
            code => UserError::Unknown(code),
        }
    }
}

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

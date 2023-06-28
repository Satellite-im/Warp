use derive_more::Display;
use serde::{Deserialize, Serialize};

//
/// `Messaging` - Allows direct, and multi-user encrypted messaging with ownership rights added so only
///             the expected users can edit, and delete messages.
///
/// `FileSystem` - Facilitates the creation of files and folders within a central directory tree (Index).
///              This index is managed internally and traversal of the directory as well as full listings,
///              deletion, and creation is provided within this module. Additionally uploading files to the filesystem.
///
/// `Accounts` - Creates a unique user accounts used to store core information about the user.
///            This can include simple things like usernames and status messages, but may also
///            include permissions, friends, and more.
///
#[derive(Hash, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Display)]
#[serde(rename_all = "lowercase")]
#[repr(C)]
pub enum Module {
    /// Allows for direct, and multi-user encrypted messaging with ownership
    #[display(fmt = "messaging")]
    Messaging,

    /// Facilitates the creation of files and directories within a central directory tree. This tree, which is an index,
    /// is managed internally and traversal of the directory as well as full listings, deletion, and creation provided within
    /// this module by an extension in addition to uploading files to the filesystem.
    #[display(fmt = "filesystem")]
    FileSystem,

    /// Creates a unique user account used to store core information about the user, which can include usernames, status messages, permissions, etc.
    #[display(fmt = "accounts")]
    Accounts,

    /// Allow for storing of data for faster access at a later point in time. Additionally, it may allow for caching of frequently used (or accessed) data
    /// so that request can be made faster.
    #[display(fmt = "cache")]
    Cache,

    /// Represents media such as audio/video calls
    #[display(fmt = "media")]
    Media,

    /// Unknown module. Should be used by default where a module cannot be identified for any specific reason.
    #[display(fmt = "unknown")]
    Unknown,
}

impl Default for Module {
    fn default() -> Self {
        Self::Unknown
    }
}

impl<A> From<A> for Module
where
    A: AsRef<str>,
{
    fn from(module: A) -> Self {
        match module.as_ref() {
            "messaging" => Module::Messaging,
            "filesystem" => Module::FileSystem,
            "accounts" => Module::Accounts,
            "cache" => Module::Cache,
            _ => Module::Unknown,
        }
    }
}

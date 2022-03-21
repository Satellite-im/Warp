use serde::{Deserialize, Serialize};
use std::fmt;

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
#[derive(Hash, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Module {
    /// Allows for direct, and multi-user encrypted messaging with ownership
    Messaging,

    /// Facilitates the creation of files and directories within a central directory tree. This tree, which is an index,
    /// is managed internally and traversal of the directory as well as full listings, deletion, and creation provided within
    /// this module by an extension in addition to uploading files to the filesystem.
    FileSystem,

    /// Creates a unique user account used to store core information about the user, which can include usernames, status messages, permissions, etc.
    Accounts,

    /// Allow for storing of data for faster access at a later point in time. Additionally, it may allow for caching of frequently used (or accessed) data
    /// so that request can be made faster.
    Cache,

    /// General identifier for the HTTP module
    Http,

    /// Manual Defining of a module
    Other(String),

    /// Unknown module. Should be used by default where a module cannot be identified for any specific reason.
    Unknown,
}

impl Default for Module {
    fn default() -> Self {
        Self::Unknown
    }
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Module::Messaging => write!(f, "MESSAGING"),
            Module::FileSystem => write!(f, "FILESYSTEM"),
            Module::Accounts => write!(f, "ACCOUNTS"),
            Module::Cache => write!(f, "CACHE"),
            Module::Http => write!(f, "HTTP"),
            Module::Other(module) => write!(f, "{module}"),
            Module::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

impl<A> From<A> for Module
where
    A: AsRef<str>,
{
    fn from(module: A) -> Self {
        match module.as_ref() {
            "MESSAGING" => Module::Messaging,
            "FILESYSTEM" => Module::FileSystem,
            "ACCOUNTS" => Module::Accounts,
            "CACHE" => Module::Cache,
            "UNKNOWN" => Module::Unknown,
            other => Module::Other(other.to_string()),
        }
    }
}

# Interface

#### Structs

```rust
/// `Role` is a representation of user-immutable data provided by a remote resource managed by the 
/// creator of the stack, this holds global roles that can be represented across the application useful
/// for validating identities of global staff members, moderators, etc.
pub struct Role {
    /// Name of the role
    name: String,

    /// TBD
    level: u8,
}

/// `Badge` is a representation of user-immutable data provided by the remote resource
/// it represents a graphical badge delegated to the user by the global authority of the app which
/// allows validating identities and participation within the global scope of the app
pub struct Badge {
    /// TBD
    name: String,

    /// TBD
    icon: String,
}


/// `Graphics` are user mutable hashes pointing to remote locations housing the users preffered
/// profile picture as well as the profile banner.
pub struct Graphics {
    /// Hash to profile picture
    profile_picture: String,

    /// Hash to profile banner
    profile_banner: String,
}

/// `Identity` encompasses all of the users identifiying information shared with the public.
/// some of this data is mutable by the user while other data within is automatically generated
/// or set by a remote authority such as Satellite.im when it comes to setting global applicaiton roles & badges.
pub struct Identity {
    /// Username of the identity
    username: String,

    /// Short 4-digit numeric id to be used along side `Identity::username` (eg `Username#0000`)
    short_id: u16,

    /// Public key for the identity
    public_key: PublicKey,

    /// TBD
    graphics: Graphics,

    /// Status message
    status_message: Option<String>,

    /// List of roles
    roles: Vec<Role>,

    /// List of available badges
    available_badges: Vec<Badge>,

    /// Active badge for identity
    active_badge: Badge,

    /// TBD
    linked_accounts: HashMap<String, String>,
}

#[derive(Default, Debug, Clone)]
pub struct Identifier {
    /// Select identity based on public key
    public_key: Option<PublicKey>,
    /// Select identity based on Username (eg `Username#0000`)
    user_name: Option<String>,
    /// Select own identity.
    own: bool,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FriendRequest {
    /// The account where the request came from
    from: PublicKey,

    /// The account where the request was sent to
    to: PublicKey,

    /// Status of the request
    status: FriendRequestStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Display)]
pub enum FriendRequestStatus {
    #[display(fmt = "uninitialized")]
    Uninitialized,
    #[display(fmt = "pending")]
    Pending,
    #[display(fmt = "accepted")]
    Accepted,
    #[display(fmt = "denied")]
    Denied,
    #[display(fmt = "friend removed")]
    FriendRemoved,
    #[display(fmt = "request removed")]
    RequestRemoved,
}

#[derive(Default, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PublicKey(Vec<u8>);

#[derive(Debug, Clone, Default)]
pub struct IdentityUpdate {
    /// Setting Username
    username: Option<String>,

    /// Path of picture
    graphics_picture: Option<String>,

    /// Path of banner
    graphics_banner: Option<String>,

    /// Setting Status Message.
    status_message: Option<Option<String>>,
}


```

#### Retrieving an Identity

Getting an identity requires an identifier to fetch by, this can either be one of two things. `PublicKey` or the `Username#0000` profile ID format.

Example identity retrieval:


```rust
Multipass::get_identity(&self, id: Identifier) -> Result<Identity>;
```

```rust
Multipass::get_own_identity(&self) -> Result<Identity> // Returns Identity
```

The function will return the `Identity` struct. After each fetch of an identity a new version of the identity will be stored in cache automatically.

#### Updating Own Identity

Allows user to update mutable identity variables such as their `Username`, `Graphics`, `Status` and more. Other values like the global `roles`, `available_badges` and more are only mutable by outside entities such as `Satellite`. These represent global application identity traits.

```rust
Multipass::update_identity(&mut self, option: IdentityUpdate) -> Result<()>;
```

The cache is updated to reflect our profile changes. This allows us to optimistically update UIs without waiting for on chain transactions to process.

#### Create Identity

This should only be called once, this is used to create a new account on the system. Calling this will store the encrypted PrivateKey on disk. Calling again will overwrite the previous account which cannot be retrieved unless the PrivateKey was backed up. The PrivateKey will be encrypted by the supplied `passphrase` so that it's not readable on disk.

```rust
Multipass::create_identity(&mut self, username: Option<&str>, passphrase: Option<&str>) -> Result<PublicKey>; // Returns PublicKey, stores encrypted private key
```

#### Decrypt Private Key

Decrypts the stored PrivateKey given a passphrase to allow interactions with the account such as on chain transactions.

```rust
Multipass::decrypt_private_key(&self, passphrase: Option<&str>) -> Result<Vec<u8>>;
```

#### Refresh Dimension Cache

Dumps local cache data for Identities ONLY. This is useful for bulk updating the cache in instances of global updates, etc.

```rust
Multipass::refresh_cache(&mut self) -> Result<()>;
````

### Friend Request/Contacts

Sending, accepting, denying or blocking requests uses a public key of an account.

***Note: `Friends` is required with `MultiPass`***

#### Sending Friend Request

```rust
  Friends::send_request(&mut self, pubkey: PublicKey) -> Result<()>;
```

#### Accepting Friend Request

```rust
  Friends::send_request(&mut self, pubkey: PublicKey) -> Result<()>;
```

#### Denying Friend Request

```rust
  Friends::deny_request(&mut self, pubkey: PublicKey) -> Result<()>;
```

#### Closing Friend Request

This will be used to close a request without accepting or denying it. 

```rust
  Friends::close_request(&mut self, pubkey: PublicKey) -> Result<()>;
```


#### List Incoming Friend Request

***Note: This will only list pending request***

```rust
  Friends::list_incoming_request(&self) -> Result<Vec<FriendRequest>>;
```


#### List Outgoing Friend Request

***Note: This will only list pending request***

```rust
  Friends::list_outgoing_request(&self) -> Result<Vec<FriendRequest>>;
```


#### List All Friend Request

***Note: This will list all request regardless of status***

```rust
  Friends::list_all_request(&self) -> Result<Vec<FriendRequest>>;
```


#### Remove Friend

```rust
  Friends::remove_friend(&mut self, pubkey: PublicKey) -> Result<()>;
```


#### Block Friend

```rust
  Friends::block_key(&mut self, pubkey: PublicKey) -> Result<()>;
```


#### List Friends


```rust
  Friends::list_friends(&self) -> Result<Vec<Identity>>;
```

#### Is Friends

This will check to see if the account is friends with the owner of the public key

```rust
  Friends::has_friend(&self, pubkey: PublicKey) -> Result<()>;
```


#### Key Exchange

This will allow the current account to perform a key exchange, allowing both the user and their friend to form a dh key for encryption

```rust
  Friends::key_exchange(&self, identity: Identity) -> Result<Vec<u8>>;
```



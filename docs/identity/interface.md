# Interface

#### Structs

```rust
/// `Role` is a representation of user-immutable data provided by a remote resource managed by the 
/// creator of the stack, this holds global roles that can be represented across the application useful
/// for validating identities of global staff members, moderators, etc.
pub struct Role {
  name: String,
  level: u8,
}

/// `Badge` is a representation of user-immutable data provided by the remote resource
/// it represents a graphical badge delegated to the user by the global authority of the app which
/// allows validating identities and participation within the global scope of the app
pub struct Badge {
  name: String,
  icon: String,
}


/// `Graphics` are user mutable hashes pointing to remote locations housing the users preffered
/// profile picture as well as the profile banner.
pub struct Graphics {
  profile_picture: String,
  profile_banner: String,
}

/// `Identity` encompasses all of the users identifiying information shared with the public.
/// some of this data is mutable by the user while other data within is automatically generated
/// or set by a remote authority such as Satellite.im when it comes to setting global applicaiton roles & badges.
pub struct Identity {
  username: String,
  short_id: u16,
  public_key: PublicKey,
  graphics: Graphics,
  status_msg: String,
  roles: Vec<Role>, // Verified is an example of a global role.
  available_badges: Vec<Badge>,
  active_badge: Badge,
  linked_accounts: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum Identifier {
    /// Select identity based on public key
    PublicKey(PublicKey),

    /// Select identity based on Username (eg `Username#0000`)
    Username(String),

    /// Select own identity.
    Own,
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
Multipass::update_identity(&mut self, id: Identifier, option: IdentityUpdate) -> Result<()>;
```

The cache is updated to reflect our profile changes. This allows us to optimistically update UIs without waiting for on chain transactions to process.

#### Create Identity

This should only be called once, this is used to create a new account on the system. Calling this will store the encrypted PrivateKey on disk. Calling again will overwrite the previous account which cannot be retrieved unless the PrivateKey was backed up. The PrivateKey will be encrypted by the supplied `passphrase` so that it's not readable on disk.

```rust
Multipass::create_identity(&mut self, username: &str, passphrase: &str) -> Result<PublicKey>; // Returns PublicKey, stores encrypted private key
```

#### Decrypt Private Key

Decrypts the stored PrivateKey given a passphrase to allow interactions with the account such as on chain transactions.

```rust
Multipass::decrypt_private_key(&self, passphrase: &str) -> Result<Vec<u8>>;
```

#### Refresh Dimension Cache

Dumps local cache data for Identities ONLY. This is useful for bulk updating the cache in instances of global updates, etc.

```rust
Multipass::refresh_cache(&mut self) -> Result<()>;
````

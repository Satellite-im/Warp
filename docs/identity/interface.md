# Interface

#### Structs

```rust
pub struct Role {
  name: String,
  level: u8,
}

pub struct Badge {
  name: String,
  icon: String,
}

pub struct Graphics {
  profile_picture: String,
  profile_banner: String,
}

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
```

#### Retrieving an Identity

Getting an identity requires an identifier to fetch by, this can either be one of two things. `PublicKey` or the `Username#0000` profile ID format.

Example identity retrieval:


```rust
Multipass::getIdentity(id: PublicKey | "Username#short_id");
```

```rust
Multipass::getOwnIdentity(); // Returns Identity
```

The response will be returned, wrapped in the `DataObject` with the payload representing the `Identity` Struct. After each fetch of an identity a new version of the identity will be stored in cache automatically.

#### Updating Own Identity

Allows user to update mutable identity variables such as their `Username`, `Graphics`, `stats_msg` and more. Other values like the global `roles`, `available_badges` and more are only mutable by outside entities such as `Satellite`. These represent global applicaiton identity traits.

```rust
Multipass::updateOwnIdentity(id: PublicKey | "Username#short_id", PartialIdentity);
```

The cache is updated to reflect our profile changes. This allows us to optimistically update UIs without waiting for on chain transactions to process.

#### Create Identity

This should only be called once, this is used to create a new account on the system. Calling this will store the encrypted PrivateKey on disk. Calling again will overwrite the previous account which cannot be retrieved unless the PrivateKey was backed up. The PrivateKey will be encrypted by the supplied `passphrase` so that it's not readable on disk.

```rust
Multipass::createIdentity(passphrase: String, identity: Identity) // Returns PublicKey, stores encrypted private key
```

#### Decrypt Private Key

Decrypts the stored PrivateKey given a passphrase to allow interactions with the account such as on chain transactions.

```rust
Multipass::decryptPrivateKey(passphrase: String);
```

#### Refresh Dimension Cache

Dumps local cache data for Identities ONLY. This is useful for bulk updating the cache in instances of global updates, etc.

```rust
Multipass::refreshCache();
````

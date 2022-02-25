# Interface

#### Structs

```rs
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


```rs
Multipass::getIdentity(id: PublicKey | "Username#short_id");
```

```rs
Multipass::getOwnIdentity(); // Returns Identity
```

The response will be returned, wrapped in the `DataObject` with the payload representing the `Identity` Struct.

#### Updating Own Identity

Allows user to update mutable identity variables such as their `Username`, `Graphics`, `stats_msg` and more. Other values like the global `roles`, `available_badges` and more are only mutable by outside entities such as `Satellite`. These represent global applicaiton identity traits.

```rs
Multipass::updateOwnIdentity(id: PublicKey | "Username#short_id", PartialIdentity);
```

#### Create Identity

This should only be called once, this is used to create a new account on the system. Calling this will store the encrypted PrivateKey on disk. Calling again will overwrite the previous account which cannot be retrieved unless the PrivateKey was backed up.

```rs
Multipass::createIdentity(passphrase: String, identity: Identity) // Returns PublicKey, stores encrypted private key
```

#### Decrypt Private Key

Decrypts the stored PrivateKey given a passphrase to allow interactions with the account such as on chain transactions.

```rs
Multipass::decryptPrivateKey(passphrase: String);
```

#### Refresh Dimension Cache

Dumps local cache data for Identities ONLY. This is useful for bulk updating the cache in instances of global updates, etc.

```rs
Multipass::refreshCache();
````

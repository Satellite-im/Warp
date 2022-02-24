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

```rs
Multipass::updateOwnIdentity(id: PublicKey | "Username#short_id", PartialIdentity);
```

#### Create Identity

```rs
Multipass::createIdentity(passphrase: String, identity: Identity) // Returns PublicKey, stores encrypted private key
```

#### Decrypt Private Key

```rs
Multipass::decryptPrivateKey(passphrase: String);
```

#### Refresh Dimension Cache

Dumps local cache data for Identities ONLY

```rs
Multipass::refreshCache();
````

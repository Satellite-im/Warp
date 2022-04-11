# Tesseract

Tesseract is an easy way to store encrypted strings for use later on, this can be user submitted API keys, locally stored keys, and more.

## Interface

***Note: It is recommended that your encryption key is not stored anywhere accessable by a third party.***

### Storing a key

Storing a key requries unlocking the store then putting a specific key into it with the `store` method. 

```rust
use warp_tesseract::{Tesseract, generate};

let mut store = Tesseract::default();

store.set(&b"MY_PASSPHRASE", "MY_KEY", "MY_VALUE").unwrap();
```

### Getting a key

Getting a key from Tesseract requires unlocking the store then retrieving a specific key stored. 
```rust
let value = store.retrieve(&b"MY_PASSPHRASE", "MY_KEY").unwrap();
```

### Loading/Saving the datastore

You can import and export the encrypted contents from and to Tesseract.

#### Saving the datastore
```rust
use warp_tesseract::{Tesseract, generate};

let mut store = Tesseract::default();

store.set(&b"MY_PASSPHRASE", "MY_KEY", "MY_VALUE").unwrap();

store.save_to_file("my_encrypted_data").unwrap();
```

#### Loading the datastore
```rust
use warp_tesseract::{Tesseract, generate};

let mut store = Tesseract::load_from_file("my_encrypted_data").unwrap();

let value = store.retrieve(&b"MY_PASSPHRASE", "MY_KEY").unwrap();
```
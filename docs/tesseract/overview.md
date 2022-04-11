# Tesseract

Tesseract is an easy way to store encrypted strings for use later on, this can be user submitted API keys, locally stored keys, and more.

## Interface

***Note: It is recommended that your encryption key is not stored anywhere accessible by a third party.***

### Unlocking Tesseract

If the `indirect` flag is used, you can unlock/lock tesseract with `Tesseract::unlock` and `Tesseract::lock`. This is required if you wish to set, retrieve, or export any contents from tesseract.

#### Unlocking Tesseract

```rust
use warp_tesseract::Tesseract;
let mut store = Tesseract::default();
store.unlock(&b"MY_PASSPHRASE").unwrap();
```

#### Locking Tesseract

***Note: Once this function is called, you would have to unlock it again. If Tesseract is also dropped out of scope, it will automatically run this function***

```rust
store.lock();
```

### Storing a key

Storing a key requries unlocking the store then putting a specific key into it with the `store` method. 

```rust
use warp_tesseract::Tesseract;

let mut store = Tesseract::default();
store.unlock(&b"MY_PASSPHRASE").unwrap();
store.set("MY_KEY", "MY_VALUE").unwrap();
```

### Getting a key

Getting a key from Tesseract requires unlocking the store then retrieving a specific key stored. 
```rust
let value = store.retrieve("MY_KEY").unwrap();
```

### Loading/Saving the datastore

You can import and export the encrypted contents from and to Tesseract.

#### Saving the datastore
```rust
use warp_tesseract::Tesseract;

let mut store = Tesseract::default();
store.unlock(&b"MY_PASSPHRASE").unwrap();
store.set("MY_KEY", "MY_VALUE").unwrap();

store.save_to_file("my_encrypted_data").unwrap();
```

#### Loading the datastore
```rust
use warp_tesseract::Tesseract;

let mut store = Tesseract::load_from_file("my_encrypted_data").unwrap();
store.unlock(&b"MY_PASSPHRASE").unwrap();
let value = store.retrieve("MY_KEY").unwrap();
```


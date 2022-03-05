# Tesseract

Tesseract is an easy way to store encrypted strings for use later on, this can be user submitted API keys, locally stored keys, and more.

## Interface

#### Unlocking Tesseract

This requires the user to pass in their `passkey`. You should **never** store this passkey locally. You should **always** pass this key directly from the user with the exception of biometric authentication methods.

```rust
Tesseract::unlock(passkey: String);
```

#### Storing a key

Storing a key requries unlocking the store then putting a specific key into it with the `store` method. 

```rust
Tesseract::unlock(passkey: String)::store(key: String, value: String);
```

#### Getting a key

Getting a key from Tesseract requires unlocking the store then retrieving a specific key stored. 
```rust
Tesseract::unlock(passkey: String)::retrieve(key: String);
```
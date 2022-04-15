# Solana MultiPass Extension Overview



## Importing extension into cargo project

In your cargo project add the following

```
[dependencies]
warp = { git = "https://github.com/Satellite-im/Warp", default-features = false, features = ["multipass"] }
warp-tesseract = { git = "https://github.com/Satellite-im/Warp", default-features = false, features = ["indirect"] } #omit `default-features = false` if you wish to use async loading/saving
```

## Starting Extension 

Here you will also need to utilize tesseract when utilizing multipass extension

```rust
    // Use development network.
    let mut account = SolanaAccount::with_devnet();
    // This is to allow `Tesseract` to be shared across multiple threads in a safe manner

    // In real world application, you would want this to be loading from a file 
    let mut tesseract = Tesseract::default();
    // This is to allow `Tesseract` to be shared across multiple threads in a safe manner
    let tesseract = Arc::new(Mutex::new(tesseract));
    tesseract.unlock(b"this is my totally secured password that should nnever be embedded in code").unwrap();
```

## Creating a new account

***Note: Second field is not used within this extension and may be subjected to removal in the future.***

```rust
    account.create_identity("NewAcctName", "").unwrap();
```

After the function successfully executes, there will be two encrypted fields within tesseract called `mnemonic` and `privkey`. 
If these fields exist and contained valid information related to a registered account, this will not be required and would
return an error.

## Fetching Account 

### Your own

```rust
    let ident = account.get_own_identity().unwrap();
```

### By Public Key
```rust
    let ident = account.get_identity(Identifer::PublicKey(PublicKey::from_bytes(....))).unwrap()
```

### By Username

**At this time, fetching by username is only available to find accounts that are cached that were previously looked up via public key.**

## Getting private key

```rust
    let privkey = account.decrypt_private_key("").unwrap();
```

This returns an array of bytes that is related for your private key. You would need to import them into the proper keypair to utilize them directly.


## StorJ Extension Overview


<img src="https://assets-global.website-files.com/602eda09fc78afc76e9706b6/621e48c4119670aeeb4309f0_storj-logo-type-ukr.svg" alt="STORJ" height="50">

[How StorJ works](https://www.storj.io/how-it-works)

### Obtaining `Access` and `Secret` keys.

**Please [review](https://docs.storj.io/dcs/getting-started/quickstart-aws-sdk-and-hosted-gateway-mt/) on how to obtain the `Access Key`, and `Secret Key`**

### Importing extension into cargo project

In your cargo project add the following

```
[dependencies]
warp = { git = "https://github.com/Satellite-im/Warp", default-features = false, features = ["constellation"] }
warp-tesseract = { git = "https://github.com/Satellite-im/Warp", default-features = false, features = ["indirect"] } #omit `default-features = false` if you wish to use async loading/saving
```

### Adding Keys to Tesseract

#### Via `warp-tesseract`

```rust
use warp_tesseract::Tesseract;

let mut tesseract = Tesseract::default();
tesseract.unlock(&b"<PASSWORD/KEY HERE>").unwrap();
tesseract.set("STORJ_ACCESS_KEY", "<ACCESS_KEY_HERE>").unwrap();
tesseract.set("STORJ_SECRET_KEY", "<SECRET_KEY_HERE>").unwrap();

//Save to a file
tesseract.to_file("datastore").unwrap();
```

You can confirm the contents of the `datastore` file by running

```rust
let mut tesseract = Tesseract::from_file(warp_directory.join("datastore")).unwrap_or_default();
tesseract.unlock(&b"<PASSWORD/KEY HERE>").unwrap();
let access_key = tesseract.retrieve("STORJ_ACCESS_KEY").unwrap();
let secret_key = tesseract.retrieve("STORJ_SECRET_KEY").unwrap();
```

#### Via Warp CLI
To add your keys to tesseract via warp command line, run 

```
warp import STORJ_ACCESS_KEY <ACCESS_KEY_HERE>
warp import STORJ_SECRET_KEY <SECRET_KEY_HERE>
```

You will be prompt for a password to use that will encrypt your keys. Once stored, you can view them by running

```
warp dump
```

This will show all the keys stored in tesseract after entering your password. 


### Testing StorJ Extension

#### Uploading Content

```rust
use warp_tesseract::Tesseract;
let mut tesseract = Tesseract::from_file(warp_directory.join("datastore")).unwrap_or_default();
tesseract.unlock(&b"<PASSWORD/KEY HERE>").unwrap();
let access_key = tesseract.retrieve("STORJ_ACCESS_KEY").unwrap();
let secret_key = tesseract.retrieve("STORJ_SECRET_KEY").unwrap();

let mut system = StorjFilesystem::new(access_key, secret_key);

system.from_buffer("new_file", &b"This is content to the file".to_vec()).await.unwrap();
```

#### Downloading Content

```rust
use warp_tesseract::Tesseract;
let mut tesseract = Tesseract::from_file(warp_directory.join("datastore")).unwrap_or_default();
tesseract.unlock(&b"<PASSWORD/KEY HERE>").unwrap();
let access_key = tesseract.retrieve("STORJ_ACCESS_KEY").unwrap();
let secret_key = tesseract.retrieve("STORJ_SECRET_KEY").unwrap();

let mut system = StorjFilesystem::new(access_key, secret_key);

let mut buffer = vec![];

system.to_buffer("new_file", &mut buffer).await.unwrap();

println!("{}", String::from_utf8_lossy(&buffer));
```

#### Sharing a link


```rust
use warp_tesseract::Tesseract;
let tesseract = Tesseract::from_file(warp_directory.join("datastore")).unwrap_or_default();
tesseract.unlock(&b"<PASSWORD/KEY HERE>").unwrap();
let access_key = tesseract.retrieve("STORJ_ACCESS_KEY").unwrap();
let secret_key = tesseract.retrieve("STORJ_SECRET_KEY").unwrap();

let mut system = StorjFilesystem::new(access_key, secret_key);

system.sync_ref().unwrap(); // To make sure the link is up to date

let file = system.current_directory().get_item("new_item").and_then(warp::item::Item::get_file).unwrap();

println!("{}", file.reference);
```

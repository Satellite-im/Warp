## IPFS Extension Overview

[Installing IPFS Desktop](https://docs.ipfs.io/install/ipfs-desktop/)
[Installing IPFS CLI](https://docs.ipfs.io/how-to/command-line-quick-start/)***Recommended***

***Note: IPFS is required to be installed and running on your machine to use this extension at this time.***

## Importing extension into cargo project

In your cargo project add the following

```
[dependencies]
warp = { git = "https://github.com/Satellite-im/Warp" }
warp-extensions = { git = "https://github.com/Satellite-im/Warp", features = ["fs_ipfs"] }
```

## Starting Extension

```rust
	use warp::constellation::Constellation;
	use warp_extensions::fs_ipfs::IpfsFileSystem;

	let mut system = IpfsFileSystem::new();
```

If you have a custom url to ipfs api server you can do the following

```rust
	let mut system = IpfsFileSystem::new_with_uri("https://127.0.0.1:5001").unwrap(); 
```

### Testing IPFS Extension

#### Upload Content

```rust
	use warp::constellation::Constellation;
	use warp_extensions::fs_ipfs::IpfsFileSystem;

	let mut system = IpfsFileSystem::new();

	system.from_buffer("new_file", &b"This is content to the file".to_vec()).await.unwrap();

```

#### Download Content

```rust
	use warp::constellation::Constellation;
	use warp_extensions::fs_ipfs::IpfsFileSystem;

	let mut system = IpfsFileSystem::new();

	let mut buffer = vec![];

	system.to_buffer("new_file", &mut buffer).await.unwrap();

	println!("{}", String::from_utf8_lossy(&buffer));

```
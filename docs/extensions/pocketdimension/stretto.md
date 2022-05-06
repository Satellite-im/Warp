## Stretto Extension Overview

Stretto is a high performance in-memory caching system. 

## Importing extension into cargo project

In your cargo project add the following

```
[dependencies]
warp = { git = "https://github.com/Satellite-im/Warp" }
warp-extensions = { git = "https://github.com/Satellite-im/Warp", features = ["pd_stretto"] }
```

## Starting Extension

```rust
	use warp::pocket_dimension::PocketDimension;
	use warp_extensions::pd_stretto::StrettoClient;

	let mut system = StrettoClient::new()?;
```

### Testing Stretto Extension

#### Add Content
***TODO***
```rust
	use warp::pocket_dimension::PocketDimension;
	use warp_extensions::pd_stretto::StrettoClient;

	let mut system = StrettoClient::new()?;
```

#### Retrieve Content
***TODO***
```rust
	use warp::pocket_dimension::PocketDimension;
	use warp_extensions::pd_stretto::StrettoClient;

	let mut system = StrettoClient::new()?;
```
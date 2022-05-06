## Flatfile Extension Overview

## Importing extension into cargo project

In your cargo project add the following

```
[dependencies]
warp = { git = "https://github.com/Satellite-im/Warp" }
warp-extensions = { git = "https://github.com/Satellite-im/Warp", features = ["pd_flatfile"] }
```

## Starting Extension

```rust
	use warp::pocket_dimension::PocketDimension;
	use warp_extensions::pd_flatfile::FlatfileStorage;

	let mut system = FlatfileStorage::new_with_index_file("</path/to/directory>", "index-file")?;
```

### Testing Flatfile Extension

#### Add Content
***TODO***
```rust
	use warp::pocket_dimension::PocketDimension;
	use warp_extensions::pd_flatfile::FlatfileStorage;

	let mut system = FlatfileStorage::new_with_index_file("</path/to/directory>", "index-file")?;
```

#### Retrieve Content
***TODO***
```rust
	use warp::pocket_dimension::PocketDimension;
	use warp_extensions::pd_flatfile::FlatfileStorage;

	let mut system = FlatfileStorage::new_with_index_file("</path/to/directory>", "index-file")?;
```
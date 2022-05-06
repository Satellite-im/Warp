## In-Memory Extension Overview

In-Memory Filesystem extension is a in-memory storage extension designed for storing and retreiving data from memory.

***
Note: This extension is used for development and testing purposes. Use of this extension will result in data loss if it is not pulled from memory.
***

***WARNING: PLEASE BE SURE TO HAVE ENOUGH AVAILABLE MEMORY BEFORE STORING ANY LARGE AMOUNTS OF DATA IN MEMORY, OTHERWISE THERE IS A CHANGE OF SYSTEM UNSTABILITY OR THE APPLICATION CRASHING DUE TO AN OUT-OF-MEMORY ERROR***

## Importing extension into cargo project

In your cargo project add the following

```
[dependencies]
warp = { git = "https://github.com/Satellite-im/Warp" }
warp-extensions = { git = "https://github.com/Satellite-im/Warp", features = ["fs_memory"] }
```

## Starting Extension

```rust
	use warp::constellation::Constellation;
	use warp_extensions::fs_memory::MemorySystem;

	let mut system = MemorySystem::new();
```

### Testing InMemory Extension

#### Upload Content

```rust
	use warp::constellation::Constellation;
	use warp_extensions::fs_memory::MemorySystem;
	
	let mut system = MemorySystem::new();

	system.from_buffer("new_file", &b"This is content to the file".to_vec()).await.unwrap();

```

#### Download Content

```rust
	use warp::constellation::Constellation;
	use warp_extensions::fs_memory::MemorySystem;
	
	let mut system = MemorySystem::new();

	let mut buffer = vec![];

	system.to_buffer("new_file", &mut buffer).await.unwrap();

	println!("{}", String::from_utf8_lossy(&buffer));

```
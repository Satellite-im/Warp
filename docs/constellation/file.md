# File

The `File` is a representation of the files uploaded to the `Constellation`. A `File` should reference an accessible location to retrieve the file that has been uploaded. 


### Creating a `File`

```rust
use warp_constellation::file::File;
    
fn main() { 
    let file = File::new("test.txt");
    assert_eq!(file.name.as_str(), "test.txt");
}
```

### Setting the description to a `File`

```rust
use warp_constellation::file::File;
    
fn main() { 
    let mut file = File::new("test.txt");
    file.set_description("Test File");

    assert_eq!(file.description.as_str(), "test file");
}
```

### Setting the size to a `File`

```rust
use warp_constellation::{file::File, item::Item};

fn main() {
    let mut file = File::new("test.txt");
    file.set_size(100000);

    assert_eq!(file.size, 100000);
}
```

### Hashing a `File`

```rust
use std::io::Cursor;
use warp_constellation::file::{File, Hash};

fn main() {
    let mut file = File::new("test.txt");

    let mut cursor = Cursor::new(b"Hello, World!");
    file.hash.sha256hash_from_reader(&mut cursor).unwrap();

    assert_eq!(file.hash.sha256, Some(String::from("DFFD6021BB2BD5B0AF676290809EC3A53191DD81C7F70A4B28688A362182986F")))
}

```
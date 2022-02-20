# Directory

The `Directory`, much like `File`, is built on top of `Item`, but include a lot more functionality than `File` that 
allow for storage of both `File` and `Directory`.

### Creating a `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let directory = Directory::new("Test Directory");

    assert_eq!(directory.name.as_str(), "Test Directory");
}
```

### Getting children from `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let mut root = Directory::new("Test Directory");
    let sub = Directory::new("Sub Directory");
    root.add_child(sub).unwrap();
    
    assert_eq!(root.has_child("Sub Directory"), true);
}
```

### Adding a child to `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let mut root = Directory::new("Test Directory");
    let sub = Directory::new("Sub Directory");
    root.add_child(sub).unwrap();
    
    assert_eq!(root.has_child("Sub Directory"), true);
}
```

### Get index of a child in a `Directory`

```rust
use warp_constellation::{
    directory::Directory,
    warp_constellation::error::Error
};

fn main() {
        let mut root = Directory::new("Test Directory");
        let sub1 = Directory::new("Sub1 Directory");
        let sub2 = Directory::new("Sub2 Directory");
        root.add_child(sub1).unwrap();
        root.add_child(sub2).unwrap();
        assert_eq!(root.get_child_index("Sub1 Directory").unwrap(), 0);
        assert_eq!(root.get_child_index("Sub2 Directory").unwrap(), 1);
        assert_eq!(root.get_child_index("Sub3 Directory").unwrap_err(), Error::ArrayPositionNotFound);
}
```

### Get a child from a `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let mut root = Directory::new("Test Directory");
    let sub = Directory::new("Sub Directory");
    root.add_child(sub).unwrap();
    assert_eq!(root.has_child("Sub Directory"), true);
    let child = root.get_child("Sub Directory").unwrap();
    assert_eq!(child.name(), "Sub Directory");
}
```

You can also get a child from a `Directory` using a specific path

```rust
use warp_constellation::{directory::Directory};

fn main() {
    let mut root = Directory::new("Test Directory");
    let mut sub0 = Directory::new("Sub Directory 1");
    let mut sub1 = Directory::new("Sub Directory 2");
    let sub2 = Directory::new("Sub Directory 3");
    sub1.add_child(sub2).unwrap();
    sub0.add_child(sub1).unwrap();
    root.add_child(sub0).unwrap();

    assert_eq!(root.get_child_by_path("/Sub Directory 1/").is_ok(), true);
    assert_eq!(root.get_child_by_path("/Sub Directory 1/Sub Directory 2/").is_ok(), true);
    assert_eq!(root.get_child_by_path("/Sub Directory 1/Sub Directory 2/Sub Directory 3").is_ok(), true);
    assert_eq!(root.get_child_by_path("/Sub Directory 1/Sub Directory 2/Sub Directory 3/Another Dir").is_ok(), false);
    
}
```
### Renaming a child within a `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
        let mut root = Directory::new("Test Directory");
        let sub = Directory::new("Sub Directory");
        root.add_child(sub).unwrap();
        assert_eq!(root.has_child("Sub Directory"), true);
    
        root.rename_child("Sub Directory", "Test Directory").unwrap();
    
        assert_eq!(root.has_child("Test Directory"), true);
}
```

### Removing a child within a `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
        let mut root = Directory::new("Test Directory");
        let sub = Directory::new("Sub Directory");
        root.add_child(sub).unwrap();
    
        assert_eq!(root.has_child("Sub Directory"), true);
        let _ = root.remove_child("Sub Directory").unwrap();
        assert_eq!(root.has_child("Sub Directory"), false);
}
```

You can also remove a child from a `Directory` using a specific path

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let mut root = Directory::new("Test Directory");
    let sub0 = Directory::new("Sub Directory 1");
    let sub1 = Directory::new("Sub Directory 2");
    let sub2 = Directory::new("Sub Directory 3");
    root.add_child(sub0).unwrap();
    root.add_child(sub1).unwrap();
    root.add_child(sub2).unwrap();
    
    assert_eq!(root.has_child("Sub Directory 1"), true);
    assert_eq!(root.has_child("Sub Directory 2"), true);
    assert_eq!(root.has_child("Sub Directory 3"), true);
    
    root.move_item_to("Sub Directory 2", "Sub Directory 1").unwrap();
    root.move_item_to("Sub Directory 3", "Sub Directory 1/Sub Directory 2").unwrap();
    
    assert_ne!(root.has_child("Sub Directory 2"), true);
    assert_ne!(root.has_child("Sub Directory 3"), true);
    
    root.remove_child_from_path("/Sub Directory 1/Sub Directory 2", "Sub Directory 3").unwrap();
    
    assert_eq!(root.get_child_by_path("Sub Directory 1/Sub Directory 2/Sub Directory 3").is_err(), true);
}
```
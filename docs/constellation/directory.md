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

### Creating recursive sub `Directory`

You can create recursive directories (similar to `mkdir -p` in unix-like environment)

```rust
use warp_constellation::{directory::Directory, item::Item};

fn main() {
    let root = Directory::new_recursive("/root/test/test2").unwrap();
    assert_eq!(root.has_item("test"), true);
    let test = root.get_item("test").and_then(Item::get_directory).unwrap();
    assert_eq!(test.has_item("test2"), true);
}
```

### Getting an item from `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let mut root = Directory::new("Test Directory");
    let sub = Directory::new("Sub Directory");
    root.add_item(sub).unwrap();
    
    assert_eq!(root.has_item("Sub Directory"), true);
}
```

### Adding a item to `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let mut root = Directory::new("Test Directory");
    let sub = Directory::new("Sub Directory");
    root.add_item(sub).unwrap();
    
    assert_eq!(root.has_item("Sub Directory"), true);
}
```

### Get index of a item in a `Directory`

Getting the item index (position) within a Directory can be useful if you wish to manually get the item from the array of items through `Directory::list_item`

```rust
use warp_constellation::{
    directory::Directory,
};
use warp_common::error::Error;

fn main() {
        let mut root = Directory::new("Test Directory");
        let sub1 = Directory::new("Sub1 Directory");
        let sub2 = Directory::new("Sub2 Directory");
        root.add_item(sub1).unwrap();
        root.add_item(sub2).unwrap();
        assert_eq!(root.get_item_index("Sub1 Directory").unwrap(), 0);
        assert_eq!(root.get_item_index("Sub2 Directory").unwrap(), 1);
        assert_eq!(root.get_item_index("Sub3 Directory").unwrap_err(), Error::ArrayPositionNotFound);
}
```

### Get a item from a `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let mut root = Directory::new("Test Directory");
    let sub = Directory::new("Sub Directory");
    root.add_item(sub).unwrap();
    assert_eq!(root.has_item("Sub Directory"), true);
    let item = root.get_item("Sub Directory").unwrap();
    assert_eq!(item.name(), "Sub Directory");
}
```

You can also get a item from a `Directory` using a specific path

```rust
use warp_constellation::{directory::Directory};

fn main() {
    let mut root = Directory::new("Test Directory");
    let mut sub0 = Directory::new("Sub Directory 1");
    let mut sub1 = Directory::new("Sub Directory 2");
    let sub2 = Directory::new("Sub Directory 3");
    sub1.add_item(sub2).unwrap();
    sub0.add_item(sub1).unwrap();
    root.add_item(sub0).unwrap();

    assert_eq!(root.get_item_by_path("/Sub Directory 1/").is_ok(), true);
    assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/").is_ok(), true);
    assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/Sub Directory 3").is_ok(), true);
    assert_eq!(root.get_item_by_path("/Sub Directory 1/Sub Directory 2/Sub Directory 3/Another Dir").is_ok(), false);
    
}
```
### Renaming a item within a `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
        let mut root = Directory::new("Test Directory");
        let sub = Directory::new("Sub Directory");
        root.add_item(sub).unwrap();
        assert_eq!(root.has_item("Sub Directory"), true);
    
        root.rename_item("Sub Directory", "Test Directory").unwrap();
    
        assert_eq!(root.has_item("Test Directory"), true);
}
```

### Removing a item within a `Directory`

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
        let mut root = Directory::new("Test Directory");
        let sub = Directory::new("Sub Directory");
        root.add_item(sub).unwrap();
    
        assert_eq!(root.has_item("Sub Directory"), true);
        let _ = root.remove_item("Sub Directory").unwrap();
        assert_eq!(root.has_item("Sub Directory"), false);
}
```

You can also remove a item from a `Directory` using a specific path

```rust
use warp_constellation::{
    directory::Directory,
};

fn main() {
    let mut root = Directory::new("Test Directory");
    let sub0 = Directory::new("Sub Directory 1");
    let sub1 = Directory::new("Sub Directory 2");
    let sub2 = Directory::new("Sub Directory 3");
    root.add_item(sub0).unwrap();
    root.add_item(sub1).unwrap();
    root.add_item(sub2).unwrap();
    
    assert_eq!(root.has_item("Sub Directory 1"), true);
    assert_eq!(root.has_item("Sub Directory 2"), true);
    assert_eq!(root.has_item("Sub Directory 3"), true);
    
    root.move_item_to("Sub Directory 2", "Sub Directory 1").unwrap();
    root.move_item_to("Sub Directory 3", "Sub Directory 1/Sub Directory 2").unwrap();
    
    assert_ne!(root.has_item("Sub Directory 2"), true);
    assert_ne!(root.has_item("Sub Directory 3"), true);
    
    root.remove_item_from_path("/Sub Directory 1/Sub Directory 2", "Sub Directory 3").unwrap();
    
    assert_eq!(root.get_item_by_path("Sub Directory 1/Sub Directory 2/Sub Directory 3").is_err(), true);
}
```
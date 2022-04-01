# Item

The `Item` represents the base for both `File` and `Directory`. It contains functions to obtain basic information like 
the name, creation 
date, as well as metadata such as the size and description of the `Item` and functions to obtain the reference 
for 
either 
`File` or `Directory` and rename the `Item`. 


### Convert from and to `Item` for both `File` and `Directory`

Both `File` and `Directory` can be converted into an `Item` due to it being the base. 

```rust
use warp_constellation::{
    directory::Directory, 
    file::File, 
    item::Item
};
 
fn main() {
    let file = File::new("test.txt");
    let directory = Directory::new("test.txt");
    
    let file_item = Item::from(file);
    let directory_item = Item::from(directory);
    
    assert_eq!(file_item.is_file(), true);
    assert_eq!(file_item.is_directory(), false);
    
    assert_eq!(directory_item.is_directory(), true);
    assert_eq!(directory_item.is_file(), false);
}
```

You are able to obtain a reference of `File` or `Directory` from an `Item`. 

```rust
use warp_constellation::{
    directory::Directory,
    file::File,
    item::Item
};

fn main() {
    {
        let file = File::new("test.txt");
        let directory = Directory::new("test.txt");
        
        let file_item = Item::from(file.clone());
        let directory_item = Item::from(directory.clone());

        assert_eq!(file_item.get_file(), Ok(&file));
        assert_eq!(directory_item.get_directory(), Ok(&directory));
    }
    {
        //You can also obtain a mutable reference of `File` or `Directory from `Item`
        let mut file = File::new("test.txt");
        let mut directory = Directory::new("test.txt");
        
        let mut file_item = Item::from(file.clone());
        let mut directory_item = Item::from(directory.clone());
        
        assert_eq!(file_item.get_file_mut(), Ok(&mut file));
        assert_eq!(directory_item.get_directory_mut(), Ok(&mut directory));
    }
}
```

### Obtain metadata information from `Item`

#### Get name from `Item`

```rust
use warp_constellation::{
    file::File, 
    item::Item
};
 
fn main() {
    let file = File::new("test.txt");
    
    let file_item = Item::from(file);
    
    assert_eq!(file_item.name(), "test.txt");
}
```

#### Get description from `Item`

```rust
use warp_constellation::{
    file::File, 
    item::Item
};
 
fn main() {
    let mut file = File::new("test.txt");
    file.set_description("Test File");
    
    let file_item = Item::from(file);
    
    assert_eq!(file_item.description(), "Test File");
}
```

#### Get size from `Item`

```rust
use warp_constellation::{
    file::File, 
    item::Item
};
 
fn main() {
    let mut file = File::new("test.txt");
    file.set_size(100);
    
    let file_item = Item::from(file);
    
    assert_eq!(file_item.size(), 100);
}
```

You can also obtain the size of a `Directory`

```rust
use warp_constellation::{
    file::File,
    directory::Directory,
    item::Item
};
 
fn main() {
    let mut file0 = File::new("test.txt");
    file.set_size(100);
    
    let mut file1 = File::new("test.txt");
    file.set_size(100);

    let mut directory = Directory::new("Test Directory");
    directory.add_item(file0).unwrap()
    directory.add_item(file1).unwrap()

    let directory_item = Item::from(directory);
    
    assert_eq!(directory_item.size(), 200);
}
```

### Altering `Item`

#### Setting the size

Note, `Item::set_size` can only be used if its value is `Item::File`, otherwise it would return an `ItemNotFile` error

```rust
use warp_constellation::{
    directory::Directory,
    file::File,
    item::Item
};

fn main() {
    let file = File::new("test.txt");
    let dir = Directory::new("Test Directory");
    
    let mut file_item = Item::from(file);
    let mut dir_item = Item::from(dir);
    
    assert_eq!(file_item.set_size(100).is_ok(), true);
    assert_eq!(dir_item.set_size(100).is_ok(), false);
}

```

#### Setting the description of an `Item`

```rust
use warp_constellation::{
    file::File,
    item::Item
};

fn main() {
    let file = File::new("test.txt");
    
    let mut file_item = Item::from(file);
    
    file_item.set_description("Test File");
    
    assert_eq!(file_item.description(), "Test File");
}
```

#### Renaming an `Item`

```rust
use warp_constellation::{
    file::File,
    item::Item
};

fn main() {
    let file = File::new("test.txt");
    
    let mut file_item = Item::from(file);
    
    file_item.rename("test.pdf").unwrap();
    
    assert_eq!(file_item.name(), "test.pdf");
}
```
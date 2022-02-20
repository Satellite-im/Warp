# File

The `File` is a representation of the files uploaded to the `Constellation`. A `File` should reference an accessible location to retrieve the file that has been uploaded. 


### Creating a `File`
```rust
use warp_constellation::file::File;
    
fn main() { 
    let file = File::new("test.txt");
    assert_eq!(file.metadata.name.as_str(), "test.txt");
}
```
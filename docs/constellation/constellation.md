# Constellation

The `Constellation` is an trait that acts similarly to `Directory`, however, it includes helpful methods to upload/download files, backup the structure, import the structure and more. 


## Filesystem

### Filesystem Structure
We would need to have a structure that would be used for our filesystem.

```rust
use warp_common::chrono::{DateTime, Utc};
use warp_common::serde::{Deserialize, Serialize};
use warp_common::{Extension, Module};
use warp_constellation::Constellation;
use warp_constellation::directory::Directory;
use warp_constellation::file::File;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "warp_common::serde")]
pub struct ExampleFileSystem {
	index: Directory,
	modified: DateTime<Utc>,
	path: PathBuf
}

impl Default for ExampleFileSystem {
    fn default() -> Self {
        DummyFileSystem {
            index: Directory::new("root"),
            modified: Utc::now(),
            path: PathBuf::new(),
        }
    }
}

```

### Implementing `Extension` and `Constellation`
Now we would need to implement `Constellation` for our struct `ExampleFileSystem`. `Extension` is also required by `Constellation` to be implemented as well.

```rust
impl Extension for ExampleFileSystem {
    fn id(&self) -> String {
        String::from("fs-example")
    }
    fn name(&self) -> String {
        String::from("Example Filesystem")
    }

    fn module(&self) -> Module {
        Module::FileSystem
    }
}

impl Constellation for ExampleFileSystem {
    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.index
    }

    fn root_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }

    fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    fn get_path(&self) -> &PathBuf {
        &self.path
    }

    fn get_path_mut(&mut self) -> &mut PathBuf {
        &mut self.path
    }
}
```

### Utilizing Filesystem functions

After everything is im place, we can now utilize the filesystem functions to access and modify (eg add/remove files or directories) the index.

```rust
// ...
let mut filesystem = ExampleFileSystem::default();

let mut root = filesystem.root_directory_mut();
root.add_item(Directory::new("test")).unwrap();

```

This will create a directory called `test` at the root of the filesystem.

### Uploading/Downloading

**Note: This would be based [warp-fs-memory](https://github.com/Satellite-im/Warp/tree/main/extensions/warp-fs-memory) extension. This also assumes you have a async system such as tokio setup**

#### Uploading a file

```rust
use warp_common::tokio;
use warp_constellation::Constellation;
use warp_fs_memory::MemorySystem;

let mut filesystem = MemorySystem::new();

let mut buf = vec![];

let mut file = tokio::fs::File::open("hello.txt").await?;
file.read_to_end(&mut buf).await?;

filesystem.from_buffer("hello.txt", &buf).await.unwrap();
```

#### Download a file

```rust
use warp_common::tokio;
use warp_constellation::Constellation;
use warp_fs_memory::MemorySystem;

let mut filesystem = MemorySystem::new();

let mut buf = vec![];

filesystem.to_buffer("test_file", &mut buf).await.unwrap();

println!("Output: {}", String::from_utf8_lossy(&buffer).to_string());
```


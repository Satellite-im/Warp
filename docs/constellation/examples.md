# Constellation Usage

Let's walk through creating a few files and folders, then navigate through them using Constellation.

This functionality is also exposed via the [Retro Relay](api/overview.md).



#### Basic Directory Use Cases

Let's start by talking about [directories](constellation/directory.md). We'll create a basic placeholder below.

```rust
use warp::{Constellation, ConstellationDataType};

let mut directory = Directory::new("SomeDirName");
// ...
```

Great, now we've got a new directory created at the root of our filesystem. Let's go ahead and add a sub directory to it!

```rust
// ...
let sub = Directory::new("Sub Directory");
directory.add_child(sub).unwrap();
// ...
```
Now we've got two directories, `SomeDirName` with a child directory of `Sub Directory`.
We can validate that our filesystem looks as expected by checking the index of the directory.

```rust
// ...
let index = root.get_child_index("SomeDirName").unwrap();
// ...
```

This will return the index of the first directory that we created. Which should contain the Sub Directory we created.
Lastly you may want to get the directory later. We can easily do that using the example below.

```rust
// ...
let mut my_test_dir = root.open_directory("SomeDirName");
```

Removing a directory will remove the contense of the directory as well as the directory itself. You can delete a directory using the example below.

```rust
// ...
```

#### Uploading Files

Uploading a file is simple, we'll use the directory we created earlier to upload a simple text file with the contents of "hello world".

```rust
```

Deleting a file is also simple.


```rust
```

#### Navigating the filesystem

It may be helpful in some use cases to be able to navigate the filesystem using Warp itself to keep track of your path. There is a helpful utility called [constellation navigation](constellation/extensions/navigation.md) which helps you do just that.


Let's first navigate into the directory we created earlier.

```rust
```

Now let's get the index of the current directory, as well as check the current path to ensure we're where we expected.

```rust
```

Now if we upload a file, it will automatically be placed inside of our current directory path.

```rust
```

Great now let's head back to the root of our constellation.

```rust
```

As you can see it's pretty straightforward but can help aliviate needing to write front end utilities to navigate through the index.
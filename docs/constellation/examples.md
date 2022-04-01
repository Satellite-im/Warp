# Constellation Usage

Let's walk through creating a few files and folders, then navigate through them using Constellation.

This functionality is also exposed via the [Retro Relay](api/overview.md).



#### Basic Directory Use Cases

Let's start by talking about [directories](constellation/directory.md). We'll create a basic placeholder below.

```rust
// ...
let mut directory = Directory::new("SomeDirName");
// ...
```

Great, now we've got a new directory created at the root of our filesystem. Let's go ahead and add a sub directory to it!

```rust
// ...
let sub = Directory::new("Sub Directory");
directory.add_item(sub)?;
// ...
```
Now we've got two directories, `SomeDirName` with a sub directory of `Sub Directory`.
We can validate that our filesystem looks as expected.

```rust
// ...
if directory.has_item("Sub Directory") { ... }
// ...
```

This will return a true if the directory we created exist.
Lastly you may want to get the directory later. We can easily do that using the example below.

```rust
// ...
let my_test_dir = directory.get_item("Sub Directory")?;
```

If you want to obtain a mutable reference of directory we can do the following

```rust
let mut my_test_dir = directory.get_item_mut("Sub Directory")?;
```

Removing a directory will remove the contense of the directory as well as the directory itself. You can delete a directory using the example below.

```rust
directory.remove_item("Sub Directory")?;
```

#### Uploading/Downloading Files

Uploading a file is simple, we'll use the directory we created earlier to upload a simple text file with the contents of "hello world".

```rust
let mut filesystem = ExampleFileSystem::new(); // Our example filesystem
filesystem.put("/Sub Directory/test.txt", "test.txt")?;
```

You can also download the file that we uploaded earlier.

```rust
filesystem.get("/Sub Directory/test.txt", "test.txt")?;
```

Deleting a file is also simple.
```rust
filesystem.remove("/Sub Directory/test.txt")?;
```

#### Navigating the filesystem

It may be helpful in some use cases to be able to navigate the filesystem using Warp itself to keep track of your path.


Let's first navigate into the directory we created earlier.

```rust
let mut filesystem = ExampleFileSystem::new(); // Our example filesystem
filesystem.select("Sub Directory")?;
```

Now let's get the current directory that we selected, as well as check the current path to ensure we're where we expected.

```rust
let current_directory = filesystem.current_directory();
if current_directory.name == "Sub Directory" { ... }
```

Now if we upload a file, it will automatically be placed inside of our current directory path.

```rust
if current_directory.has_item("test.txt") { ... }
```

Great now let's head back to the root of our constellation.

```rust
filesystem.go_back()?;
```

As you can see it's pretty straightforward but can help aliviate needing to write front end utilities to navigate through the index.
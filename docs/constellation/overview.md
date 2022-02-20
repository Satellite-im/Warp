# Constellation (FileSystem Module)

Constellation (FileSystem) is in charge of keeping track of all directories and files on the ecosystem. In addition, 
it sends off the request to the module implementation to upload, clone, and delete files. The reasoning is that if 
we build a file structure in our service we do not need to worry about using one of the (very few) protocols which actually neatly keep track of files. Instead, we can simply expect a location hash, or some other way to fetch the file while Warp does all the leg work for tracking and organizing the files. 


The `Constellation` acts similarly to a `Directory`, however, it includes helpful methods to create clones, backup the 
structure, import the structure and more. It should be used to interface with `Files` and `Directory` as opposed to 
interfacing with them directly (in most cases).

#### File

The `File` is a representation of the files uploaded to the `Constellation`. A `File` should reference an accessible location to retrieve the file that has been uploaded. 

[More about File](constellation/file.md)...

#### Directory

The `Directory`, much like `File`, is built on top of `Item`, but include a lot more functionality than `File` that
allow for storage of both `File` and `Directory`.

[More about Directory](constellation/directory.md)...


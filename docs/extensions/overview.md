# Extensions at a Glance

Warp is built on top of extensions. Each  extension registers itself with the service and extends it's functionality. At it's core Warp just brings these extensions together.

Each  extension has its own abstract interface, these interfaces are extended with  extension runners.  extension runners can be swapped out without changes to the end products using the service, by design, because each runner will use the same interface. The runner is not, and should not, be important to the end user / service of Warp since it's abstracted behind the abstract interface.

The goal of this is to allow Satellite.im to evolve as technology changes in the space and better options appear with very minimal additional development outside of Warp for core functionalities.

#### Included  extension Interfaces

**Constellation** - Facilitates the creation of files and folders within a central directory tree (Index). This 
index is managed internally and traversal of the directory as well as full listings, deletion, and creation is provided within this  extension. Additionally, uploading files to the filesystem.

**PocketDimension** - Store data for quick indexing and searching, which may be useful to cache frequently used data so that request can be made faster.

**Messaging** - Allows direct, and multi-user encrypted messaging with ownership rights added so only the expected users can edit, and delete messages. 

**Accounts** - Creates a unique user accounts used to store core information about the user. This can include simple things like usernames and status messages, but may also include permissions, friends, and more.
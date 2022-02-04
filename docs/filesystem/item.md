# Item

The `Item` represents the base for both `Files` and `Folders`. It contains basic information like the name, creation date, parent, as well as metadata such as the size of the `Item`.

```rs
pub struct ItemMeta {
  pub id: String,       // UUIDv4 identifier
  pub name: String,     // Name of the item
  pub size: String,     // Size in bytes of item
}

pub trait Item for ItemMeta {
  fn path(&self) -> String;                                          // Location of item inside the FileSystem
  fn clone(&self) -> Item;                                           // Create a carbon copy, but with a new ID
  fn set_name(&mut self, name: String) -> Result<String, Error>;     // Updates the name of the item
  fn set_parent(&mut self, parent: Directory) -> Result<(), Error>;  // Update the parent, effectivley moving the item
}
```
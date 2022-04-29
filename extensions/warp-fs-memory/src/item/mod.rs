use crate::item::directory::Directory;
use crate::item::file::File;
use chrono::{DateTime, Utc};
use dyn_clone::DynClone;
use std::fmt::Debug;

pub mod directory;
pub mod file;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ItemType {
    File,
    Directory,
}

pub trait Item: DynClone + Debug + Sync + Send {
    fn name(&self) -> String;

    fn r#type(&self) -> ItemType;

    fn create(&self) -> DateTime<Utc>;

    fn size(&self) -> usize;

    fn to_directory(&self) -> crate::Result<&Directory>;

    fn hash(&self) -> Vec<u8> {
        Vec::new()
    }

    fn data(&self) -> Vec<u8> {
        Vec::new()
    }

    fn to_directory_mut(&mut self) -> crate::Result<&mut Directory>;

    fn to_file(&self) -> crate::Result<&File>;

    fn to_file_mut(&mut self) -> crate::Result<&mut File>;
}

pub trait ItemMut: Item {
    fn as_mut(&mut self) -> &mut Self;
}

impl PartialEq for Box<dyn Item> {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name() && self.hash() == other.hash()
    }
}

impl Eq for Box<dyn Item> {}

dyn_clone::clone_trait_object!(Item);

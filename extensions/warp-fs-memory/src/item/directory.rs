use warp::error::Error;

use crate::item::file::File;
use crate::item::{ItemMut, ItemType};
use crate::Item;
use anyhow::bail;
use chrono::{DateTime, Utc};
use uuid::Uuid;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directory {
    pub id: Uuid,
    pub name: String,
    pub create: DateTime<Utc>,
    pub content: Vec<Box<dyn Item>>,
}

impl Item for Directory {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn r#type(&self) -> ItemType {
        ItemType::Directory
    }

    fn create(&self) -> DateTime<Utc> {
        self.create
    }

    fn size(&self) -> usize {
        self.content.iter().map(|i| i.size()).sum()
    }

    fn to_directory(&self) -> crate::Result<&Directory> {
        Ok(self)
    }

    fn to_directory_mut(&mut self) -> crate::Result<&mut Directory> {
        Ok(self)
    }

    fn to_file(&self) -> crate::Result<&File> {
        Err(Error::Other)
    }

    fn to_file_mut(&mut self) -> crate::Result<&mut File> {
        Err(Error::Other)
    }
}

impl ItemMut for Directory {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl From<Directory> for Box<dyn Item> {
    fn from(dir: Directory) -> Self {
        Box::new(dir)
    }
}

impl Directory {
    pub fn new<S: AsRef<str>>(name: S) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.as_ref().to_string(),
            create: Utc::now(),
            content: Vec::new(),
        }
    }
    pub fn new_recursive<S: AsRef<str>>(path: S) -> anyhow::Result<Self> {
        let path = path.as_ref();

        // Check to determine if the string is initially empty
        if path.is_empty() {
            bail!("Path provided is invalid");
        }

        let mut path = path
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();

        // checl to determine if the array is empty
        if path.is_empty() {
            bail!("Path provided is invalid");
        }

        let name = path.remove(0);
        let mut directory = Self::new(name);
        if !path.is_empty() {
            let sub = Self::new_recursive(path.join("/"))?;
            directory.insert(sub)?;
        }
        Ok(directory)
    }

    pub fn get_content(&self) -> &Vec<Box<dyn Item>> {
        &self.content
    }

    pub fn get_content_mut(&mut self) -> &mut Vec<Box<dyn Item>> {
        &mut self.content
    }

    pub fn insert<I: Into<Box<dyn Item>>>(&mut self, item: I) -> crate::Result<()> {
        let item = item.into();
        if self.exist(item.name().as_ref()) {
            return Err(Error::Other);
        }
        self.content.push(item);
        Ok(())
    }

    pub fn remove<S: AsRef<str>>(&mut self, name: S) -> crate::Result<Box<dyn Item>> {
        let index = self
            .content
            .iter()
            .position(|item| item.name() == *name.as_ref())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        let item = self.content.remove(index);

        Ok(item)
    }

    pub fn exist(&self, name: &str) -> bool {
        self.content
            .iter()
            .filter(|item| item.name() == *name)
            .count()
            == 1
    }

    #[allow(clippy::borrowed_box)]
    pub fn get_item(&self, name: &str) -> crate::Result<&Box<dyn Item>> {
        if !self.exist(name) {
            return Err(Error::Other);
        }
        let index = self
            .content
            .iter()
            .position(|item| item.name() == *name)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        self.content.get(index).ok_or(Error::Other)
    }

    #[allow(clippy::borrowed_box)]
    pub fn get_item_from_path<S: AsRef<str>>(&self, path: S) -> crate::Result<&Box<dyn Item>> {
        let mut path = path
            .as_ref()
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();
        if path.is_empty() {
            return Err(Error::IoError(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )));
        }
        let name = path.remove(0);
        let item = self.get_item(name)?;
        return if !path.is_empty() {
            if item.r#type() == ItemType::Directory {
                item.to_directory()?.get_item_from_path(path.join("/"))
            } else {
                Ok(item)
            }
        } else {
            Ok(item)
        };
    }

    pub fn move_item_to<S: AsRef<str>>(&mut self, child: S, dst: S) -> anyhow::Result<()> {
        let (child, dst) = (child.as_ref().trim(), dst.as_ref().trim());

        if self.get_item_from_path(dst)?.r#type() == ItemType::File {
            bail!("Destination is not a directory");
        }

        if self.get_item_from_path(dst)?.to_directory()?.exist(child) {
            bail!("Item exist in destination");
        }

        let item = self.remove(child)?;

        // TODO: Implement check and restore item back to previous directory if there's an error
        self.get_item_mut_from_path(dst)
            .and_then(|item| item.to_directory_mut())?
            .insert(item)?;

        Ok(())
    }

    pub fn get_item_mut(&mut self, name: &str) -> crate::Result<&mut Box<dyn Item>> {
        if !self.exist(name) {
            return Err(Error::Other);
        }
        let index = self
            .content
            .iter()
            .position(|item| item.name() == *name)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;

        self.content.get_mut(index).ok_or(Error::Other)
    }

    pub fn get_item_mut_from_path<S: AsRef<str>>(
        &mut self,
        path: S,
    ) -> crate::Result<&mut Box<dyn Item>> {
        let mut path = path
            .as_ref()
            .split('/')
            .filter(|&s| !s.is_empty())
            .collect::<Vec<_>>();
        if path.is_empty() {
            return Err(Error::IoError(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )));
        }
        let name = path.remove(0);
        let item = self.get_item_mut(name)?;
        return if !path.is_empty() {
            if item.r#type() == ItemType::Directory {
                item.to_directory_mut()?
                    .get_item_mut_from_path(path.join("/"))
            } else {
                Ok(item)
            }
        } else {
            Ok(item)
        };
    }
}

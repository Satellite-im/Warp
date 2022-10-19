use warp::error::Error;

use crate::item::directory::Directory;
use crate::item::{ItemMut, ItemType};
use crate::Item;
use chrono::{DateTime, Utc};

use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub id: Uuid,
    pub name: String,
    pub create: DateTime<Utc>,
    pub data: Vec<u8>,
    pub hash: Vec<u8>,
}

impl From<File> for Box<dyn Item> {
    fn from(file: File) -> Self {
        Box::new(file)
    }
}

impl Item for File {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn r#type(&self) -> ItemType {
        ItemType::File
    }

    fn create(&self) -> DateTime<Utc> {
        self.create
    }

    fn rename(&mut self, name: &str) {
        if self.name == name {
            return;
        }
        self.name = name.to_string();
    }

    fn hash(&self) -> Vec<u8> {
        self.hash.to_vec()
    }

    fn data(&self) -> Vec<u8> {
        self.data.to_vec()
    }

    fn size(&self) -> usize {
        self.data.len()
    }

    fn to_directory(&self) -> crate::Result<&Directory> {
        Err(Error::Other)
    }

    fn to_directory_mut(&mut self) -> crate::Result<&mut Directory> {
        Err(Error::Other)
    }

    fn to_file(&self) -> crate::Result<&File> {
        Ok(self)
    }

    fn to_file_mut(&mut self) -> crate::Result<&mut File> {
        Ok(self)
    }
}

impl ItemMut for File {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl File {
    pub fn new(name: &str) -> File {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            create: Utc::now(),
            data: vec![],
            hash: vec![],
        }
    }

    pub fn valid(&self) -> bool {
        false
    }

    pub fn insert_buffer(&mut self, data: Vec<u8>) -> crate::Result<usize> {
        self.data = data;
        Ok(self.data.len())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl File {
    pub fn insert_stream<R: std::io::Read>(&mut self, reader: &mut R) -> crate::Result<usize> {
        let mut buf = vec![];
        let mut size = 0;
        loop {
            match reader.read(&mut buf[size..]) {
                Ok(0) => break,
                Ok(n) => size += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(warp::error::Error::from(e)),
            }
        }

        self.data = buf;

        Ok(size)
    }

    pub fn insert_from_path<P: AsRef<std::path::Path>>(&mut self, path: P) -> crate::Result<usize> {
        let data = std::fs::read(path)?;

        self.insert_buffer(data)
    }
}

use crate::error::Error;
use crate::item::directory::Directory;
use crate::item::{ItemMut, ItemType};
use crate::Item;
use blake2::{Blake2b512, Digest};
use std::fs;
use std::io::Read;
use std::path::Path;
use warp_common::chrono::{DateTime, Utc};
use warp_common::uuid::Uuid;

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

    pub(crate) fn hash_data(&mut self) -> crate::Result<()> {
        let mut hasher = Blake2b512::new();
        hasher.update(&self.data[..]);
        let res = hasher.finalize().to_vec();
        self.hash = res;
        Ok(())
    }

    pub fn valid(&self) -> bool {
        false
    }

    pub fn insert_stream<R: Read>(&mut self, reader: &mut R) -> crate::Result<usize> {
        let mut buf = vec![];
        let mut size = 0;
        loop {
            match reader.read(&mut buf[size..]) {
                Ok(0) => break,
                Ok(n) => size += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::from(e)),
            }
        }

        self.data = buf;

        match self.hash_data() {
            Ok(()) => {}
            Err(e) => {
                self.data.clear();
                return Err(e);
            }
        };

        Ok(size)
    }

    pub fn insert_buffer(&mut self, data: Vec<u8>) -> crate::Result<usize> {
        self.data = data;
        match self.hash_data() {
            Ok(()) => {}
            Err(e) => {
                self.data.clear();
                return Err(e);
            }
        };
        Ok(self.data.len())
    }

    pub fn insert_from_path<P: AsRef<Path>>(&mut self, path: P) -> crate::Result<usize> {
        let data = fs::read(path)?;

        self.insert_buffer(data)
    }
}

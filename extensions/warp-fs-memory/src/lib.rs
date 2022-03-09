pub mod error;
pub mod item;

use item::Item;
use warp_common::error::Error;
use warp_data::DataObject;
use warp_pocket_dimension::query::QueryBuilder;
use std::io::{Read, Write, ErrorKind};
use warp_common::chrono::{DateTime, Utc};
use warp_common::serde::{Deserialize, Serialize};
use warp_constellation::constellation::{Constellation, ConstellationGetPut, ConstellationVersion};
use warp_constellation::directory::Directory;
use warp_module::Module;

pub type Result<T> = std::result::Result<T, error::Error>;

#[derive(Debug)]
pub struct MemorySystemInternal(item::directory::Directory);


impl Default for MemorySystemInternal {
    fn default() -> Self {
        MemorySystemInternal(item::directory::Directory::new("root"))
    }
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(crate = "warp_common::serde")]
pub struct MemorySystem {
    version: ConstellationVersion,
    index: Directory,
    modified: DateTime<Utc>,
    #[serde(skip)]
    internal: MemorySystemInternal,
}

impl MemorySystemInternal {
    pub fn new() -> Self {
        MemorySystemInternal::default()
    }
}

impl Constellation for MemorySystem {
    fn version(&self) -> &ConstellationVersion {
        &self.version
    }

    fn modified(&self) -> DateTime<Utc> {
        self.modified
    }

    fn root_directory(&self) -> &Directory {
        &self.index
    }

    fn root_directory_mut(&mut self) -> &mut Directory {
        &mut self.index
    }
}

impl ConstellationGetPut for MemorySystem {
    fn put<R: Read, S: AsRef<str>, C: warp_pocket_dimension::PocketDimension>(
        &mut self,
        name: S,
        cache: &mut C,
        reader: &mut R,
    ) -> std::result::Result<(), warp_common::error::Error> {
        //TODO: Autocreate directories if there is a path used and directories are non-existent

        let mut internal_file = item::file::File::new(name.as_ref());
        let bytes = internal_file.insert_stream(reader).unwrap();
        self.internal.0.insert(internal_file.clone()).map_err(|_| Error::Other)?;
        let mut data = DataObject::default();
        data.set_size(bytes as u64);
        data.set_payload((name.as_ref().to_string(), internal_file.data()))?;

        let mut file = warp_constellation::file::File::new(name.as_ref());
        file.set_size(bytes as i64);
        file.set_hash(hex::encode(internal_file.hash()));

        self.open_directory("")?.add_child(file)?;
        cache.add_data(Module::FileSystem, &data)?;
        Ok(())
    }

    fn get<W: Write, S: AsRef<str>, C: warp_pocket_dimension::PocketDimension>(
        &self,
        name: S,
        cache: &C,
        writer: &mut W,
    ) -> std::result::Result<(), warp_common::error::Error> {
        let name = name.as_ref();

        //temporarily make it mutable
        if !self.root_directory().has_child(name) {
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidData,
            )));
        }

        let mut query = QueryBuilder::default();
        query.r#where("name", name.to_string())?;
        match cache.get_data(Module::FileSystem, Some(&query)) {
            Ok(d) => {
                //get last
                if !d.is_empty() {
                    let mut list = d.clone();
                    let obj = list.pop().unwrap();
                    let (in_name, buf) = obj.payload::<(String, Vec<u8>)>()?;
                    if name != in_name {
                        return Err(Error::Other);// mismatch with names
                    } 
                    writer.write_all(&buf)?;
                    writer.flush()?;
                    return Ok(());
                }
            }
            Err(_) => {}
        }

        let file = self.internal.0.get_item_from_path(name.to_string()).map_err(|_| Error::Other)?;

        writer.write_all(&file.data())?;
        writer.flush()?;
        
        Ok(())
    }
}

//TODO
// #[cfg(test)]
// mod test {
//     use crate::item::directory::Directory;
//     use crate::item::file::File;
//     use crate::MemorySystemInternal;
//     use warp_common::anyhow;

//     #[test]
//     fn testing() -> anyhow::Result<()>{
//         let mut memory = MemorySystemInternal::new();

//         let mut dir = Directory::new("Test");

//         let mut file = File::new("test.txt");
//         file.insert_from_path("test.txt").unwrap();

//         dir.insert(file).unwrap();

//         Ok(())
//     }
// }

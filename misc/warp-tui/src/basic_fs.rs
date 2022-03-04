use crate::HOOKS;
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::ops::Index;
use warp_common::chrono::{DateTime, Utc};
use warp_common::error::Error;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::ExtensionInfo;
use warp_constellation::constellation::{Constellation, ConstellationGetPut, ConstellationVersion};
use warp_constellation::directory::Directory;
use warp_constellation::file::File;
use warp_data::DataObject;
use warp_module::Module;
use warp_pocket_dimension::query::QueryBuilder;

#[derive(Debug, Default, Clone)]
pub struct BasicSystemInternal(HashMap<String, Vec<u8>>);

impl ExtensionInfo for BasicFileSystem {
    fn name(&self) -> String {
        String::from("Basic Filesystem")
    }

    fn description(&self) -> String {
        String::from("Basic in-memory filesystem")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(crate = "warp_common::serde")]
pub struct BasicFileSystem {
    pub version: ConstellationVersion,
    pub index: Directory,
    pub modified: DateTime<Utc>,
    #[serde(skip)]
    pub memory: BasicSystemInternal,
}

impl Default for BasicFileSystem {
    fn default() -> Self {
        BasicFileSystem {
            version: ConstellationVersion::from((0, 1, 2)),
            index: Directory::new("root"),
            modified: Utc::now(),
            memory: BasicSystemInternal::default(),
        }
    }
}

impl Constellation for BasicFileSystem {
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

impl ConstellationGetPut for BasicFileSystem {
    fn put<R: Read, S: AsRef<str>, C: warp_pocket_dimension::PocketDimension>(
        &mut self,
        name: S,
        cache: &mut C,
        reader: &mut R,
    ) -> std::result::Result<(), warp_common::error::Error> {
        let name = name.as_ref();
        let mut buf = vec![];

        let size = reader.read_to_end(&mut buf)?;
        if size == 0 {
            return Err(warp_common::error::Error::IoError(std::io::Error::from(
                ErrorKind::InvalidData,
            )));
        }

        self.memory.0.insert(name.to_string(), buf.clone());

        let mut data = DataObject::new(&Module::FileSystem, (name.to_string(), buf))?;
        data.size = size as u64;

        cache.add_data(Module::FileSystem, &data)?;

        self.open_directory("")?.add_child(File::new(name))?;
        HOOKS.lock().unwrap().trigger("FILESYSTEM::NEW_FILE", &data);
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
                        return Err(Error::Other);
                    } // mismatch with names
                    writer.write_all(&buf)?;
                    writer.flush()?;
                    return Ok(());
                }
            }
            Err(e) => {}
        }

        let data = self.memory.0.get(name).ok_or(Error::Other)?;

        writer.write_all(&data)?;
        writer.flush()?;
        Ok(())
    }
}

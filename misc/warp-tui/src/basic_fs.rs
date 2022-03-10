// use crate::HOOKS;
// use blake2::{Blake2b512, Digest};
// use warp_pocket_dimension::PocketDimension;
// use std::collections::HashMap;
// use std::io::{ErrorKind, Read, Write};
// use std::sync::{Arc, Mutex};
// use warp_common::chrono::{DateTime, Utc};
// use warp_common::error::Error;
// use warp_common::serde::{Deserialize, Serialize};
// use warp_common::Extension;
// use warp_constellation::constellation::{Constellation, ConstellationGetPut, ConstellationVersion, ConstellationImportExport};
// use warp_constellation::directory::Directory;
// use warp_constellation::file::File;
// use warp_data::DataObject;
// use warp_module::Module;
// use warp_pocket_dimension::query::QueryBuilder;

// #[derive(Debug, Default, Clone)]
// pub struct BasicSystemInternal(HashMap<String, Vec<u8>>);

// impl Extension for BasicFileSystem {
//     fn name(&self) -> String {
//         String::from("Basic Filesystem")
//     }

//     fn description(&self) -> String {
//         String::from("Basic in-memory filesystem")
//     }
// }

// #[derive(Serialize, Deserialize, Clone)]
// #[serde(crate = "warp_common::serde")]
// pub struct BasicFileSystem {
//     pub version: ConstellationVersion,
//     pub index: Directory,
//     pub modified: DateTime<Utc>,
//     #[serde(skip)]
//     pub memory: BasicSystemInternal,
//     #[serde(skip)]
//     pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>
// }

// impl Default for BasicFileSystem {
//     fn default() -> Self {
//         BasicFileSystem {
//             version: ConstellationVersion::from((0, 1, 2)),
//             index: Directory::new("root"),
//             modified: Utc::now(),
//             memory: BasicSystemInternal::default(),
//             cache: None,
//         }
//     }
// }


// impl BasicFileSystem {
//     pub fn new(cache: Arc<Mutex<Box<dyn PocketDimension>>>) -> Self {
//         BasicFileSystem {
//             version: ConstellationVersion::from((0, 1, 2)),
//             index: Directory::new("root"),
//             modified: Utc::now(),
//             memory: BasicSystemInternal::default(),
//             cache: Some(cache)
//         }
//     }
// }

// impl Constellation for BasicFileSystem {
//     fn version(&self) -> &ConstellationVersion {
//         &self.version
//     }

//     fn modified(&self) -> DateTime<Utc> {
//         self.modified
//     }

//     fn root_directory(&self) -> &Directory {
//         &self.index
//     }

//     fn root_directory_mut(&mut self) -> &mut Directory {
//         &mut self.index
//     }
// }

// impl ConstellationGetPut for BasicFileSystem {
//     fn put<R: Read>(
//         &mut self,
//         name: &str,
//         reader: &mut R,
//     ) -> std::result::Result<(), warp_common::error::Error> {
//         let mut buf = vec![];

//         let size = reader.read_to_end(&mut buf)?;
//         if size == 0 {
//             return Err(warp_common::error::Error::IoError(std::io::Error::from(
//                 ErrorKind::InvalidData,
//             )));
//         }

//         self.memory.0.insert(name.to_string(), buf.clone());

//         let mut hasher = Blake2b512::new();
//         hasher.update(&buf[..]);
//         let res = hasher.finalize().to_vec();

//         let mut data = DataObject::new(&Module::FileSystem, (name.to_string(), buf))?;
//         data.size = size as u64;

//         let mut file = File::new(name);
//         file.set_size(size as i64);
//         file.set_hash(hex::encode(res));

//         self.open_directory("")?.add_child(file)?;
//         self.cache.as_ref().unwrap().lock().unwrap().add_data(Module::FileSystem, &data)?;
//         HOOKS.lock().unwrap().trigger("FILESYSTEM::NEW_FILE", &data);
//         Ok(())
//     }

//     fn get<W: Write>(
//         &self,
//         name: &str,
//         writer: &mut W,
//     ) -> std::result::Result<(), warp_common::error::Error> {

//         //temporarily make it mutable
//         if !self.root_directory().has_child(name) {
//             return Err(warp_common::error::Error::IoError(std::io::Error::from(
//                 ErrorKind::InvalidData,
//             )));
//         }

//         let mut query = QueryBuilder::default();
//         query.r#where("name", name.to_string())?;

//         match self.cache.as_ref().unwrap().lock().unwrap().get_data(Module::FileSystem, Some(&query)) {
//             Ok(d) => {
//                 //get last
//                 if !d.is_empty() {
//                     let mut list = d.clone();
//                     let obj = list.pop().unwrap();
//                     let (in_name, buf) = obj.payload::<(String, Vec<u8>)>()?;
//                     if name != in_name {
//                         return Err(Error::Other);
//                     } // mismatch with names
//                     writer.write_all(&buf)?;
//                     writer.flush()?;
//                     return Ok(());
//                 }
//             }
//             Err(_) => {}
//         }

//         let data = self.memory.0.get(name).ok_or(Error::Other)?;

//         writer.write_all(&data)?;
//         writer.flush()?;
//         Ok(())
//     }
// }

// impl ConstellationImportExport for BasicFileSystem {}
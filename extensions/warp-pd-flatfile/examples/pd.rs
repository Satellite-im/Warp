use std::path::PathBuf;

use warp_common::error::Error;
use warp_common::{anyhow, Module};
use warp_data::{DataObject, DataType};
use warp_pd_flatfile::FlatfileStorage;
use warp_pocket_dimension::query::{Comparator, QueryBuilder};
use warp_pocket_dimension::{DimensionData, PocketDimension};

fn main() -> anyhow::Result<()> {
    let mut root = std::env::temp_dir();
    root.push("pd-cache");

    let index = {
        let mut index = PathBuf::new();
        index.push("cache-index");

        index
    };

    let mut storage = FlatfileStorage::new_with_index_file(root, index)?;

    let data = DataObject::new(
        DataType::Module(Module::FileSystem),
        DimensionData::from_path("Cargo.toml"),
    )?;

    storage.add_data(DataType::Module(Module::FileSystem), &data)?;

    let bufdata = DataObject::new(
        DataType::Module(Module::FileSystem),
        DimensionData::from_buffer_nofile("testbin", b"Hello, World"),
    )?;

    storage.add_data(DataType::Module(Module::FileSystem), &bufdata)?;
    let bufdata = DataObject::new(
        DataType::Module(Module::FileSystem),
        DimensionData::from_buffer_nofile("test", b"Hello, World"),
    )?;
    storage.add_data(DataType::File, &bufdata)?;

    let mut query = QueryBuilder::default();
    query.filter(Comparator::Eq, "name", "testbin")?;

    let arr = storage
        .get_data(DataType::Module(Module::FileSystem), Some(&query))?
        .last()
        .ok_or(Error::InvalidDataType)?
        .payload::<DimensionData>()?;

    let mut buf: Vec<u8> = vec![];

    arr.write_from_path(&mut buf)?;

    println!("Contents: {}", String::from_utf8_lossy(&buf));

    storage.empty(DataType::Module(Module::FileSystem))?;
    Ok(())
}

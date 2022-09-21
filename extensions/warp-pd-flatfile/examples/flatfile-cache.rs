use std::path::PathBuf;
use warp::data::{DataType};
use warp::error::Error;
use warp::module::Module;
use warp::pocket_dimension::query::{Comparator, QueryBuilder};
use warp::pocket_dimension::{DimensionData, PocketDimension};
use warp::sata::Sata;
use warp_pd_flatfile::FlatfileStorage;

fn main() -> anyhow::Result<()> {
    let mut root = std::env::temp_dir();
    root.push("pd-cache");

    let index = {
        let mut index = PathBuf::new();
        index.push("cache-index");

        index
    };

    let mut storage = FlatfileStorage::new_with_index_file(root, index)?;
    let data = Sata::default().encode(
        warp::libipld::IpldCodec::DagCbor,
        warp::sata::Kind::Reference,
        DimensionData::from("Cargo.toml"),
    )?;

    let data_account = Sata::default();

    storage.add_data(DataType::from(Module::FileSystem), &data)?;
    storage.add_data(DataType::from(Module::Accounts), &data_account)?;

    let bufdata = Sata::default().encode(
        warp::libipld::IpldCodec::DagCbor,
        warp::sata::Kind::Reference,
        DimensionData::from_buffer_nofile("testbin", b"Hello, World"),
    )?;

    storage.add_data(DataType::from(Module::FileSystem), &bufdata)?;

    let bufdata = Sata::default().encode(
        warp::libipld::IpldCodec::DagCbor,
        warp::sata::Kind::Reference,
        DimensionData::from_buffer_nofile("test", b"Hello, World"),
    )?;

    storage.add_data(DataType::FileSystem, &bufdata)?;

    let mut query = QueryBuilder::default();
    query.filter(Comparator::Eq, "name", "testbin")?;

    let arr = storage
        .get_data(DataType::from(Module::FileSystem), Some(&query))?
        .last()
        .ok_or(Error::InvalidDataType)?
        .decode::<DimensionData>()?;

    let mut buf: Vec<u8> = vec![];

    arr.write_from_path(&mut buf)?;

    println!("Contents: {}", String::from_utf8_lossy(&buf));
    
    storage.empty(DataType::from(Module::FileSystem))?;
    storage.empty(DataType::from(Module::Accounts))?;
    Ok(())
}

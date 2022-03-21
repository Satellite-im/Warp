use std::path::{Path, PathBuf};

use warp_common::{anyhow, Module};
use warp_data::{DataObject, DataType};
use warp_pd_flatfile::FlatfileStorage;
use warp_pocket_dimension::query::{Comparator, QueryBuilder};
use warp_pocket_dimension::PocketDimension;

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
        &DataType::Module(Module::FileSystem),
        Path::new("Cargo.toml").to_path_buf(),
    )?;

    storage.add_data(DataType::Module(Module::FileSystem), &data)?;

    let mut query = QueryBuilder::default();
    query.filter(Comparator::Eq, "name", "Cargo.toml")?;

    let arr = storage.size(DataType::Module(Module::FileSystem), None)?;

    println!("{}", arr);
    Ok(())
}

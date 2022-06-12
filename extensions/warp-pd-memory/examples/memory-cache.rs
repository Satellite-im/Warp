use std::path::PathBuf;

use serde::Deserialize;
use warp::data::{DataObject, DataType};
use warp::error::Error;
use warp::module::Module;
use warp::pocket_dimension::query::{Comparator, QueryBuilder};
use warp::pocket_dimension::{DimensionData, PocketDimension};
use warp_pd_memory::MemoryClient;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub port: u16,
    pub data: String,
}

fn main() -> anyhow::Result<()> {
    let items = vec![
        Item {
            port: 10000,
            data: "Local".into(),
        },
        Item {
            port: 10001,
            data: "Global".into(),
        },
        Item {
            port: 10002,
            data: "All".into(),
        },
    ];

    let mut cache = MemoryClient::new();

    for item in items {
        let data = DataObject::new(DataType::from(Module::Unknown), item)?;

        cache.add_data(DataType::from(Module::Unknown), &data)?;
    }

    let mut query = QueryBuilder::default();
    query.filter(Comparator::Eq, "port", 10001)?;

    let data_list = cache.get_data(DataType::from(Module::Unknown), Some(&query))?;

    for data in data_list {
        let item = data.payload::<Item>()?;
        println!("Item::port={}", item.port);
        println!("Item::port={}", item.port);
    }

    cache.empty(DataType::from(Module::FileSystem))?;
    Ok(())
}

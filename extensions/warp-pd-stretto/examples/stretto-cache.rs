use warp::data::DataType;
use warp::module::Module;
use warp::pocket_dimension::query::{Comparator, QueryBuilder};
use warp::pocket_dimension::PocketDimension;
use warp::sata::Sata;
use warp_pd_stretto::StrettoClient;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub port: i64,
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

    let mut cache = StrettoClient::new()?;

    for item in items {
        let data = Sata::default().encode(
            warp::sata::libipld::IpldCodec::DagCbor,
            warp::sata::Kind::Reference,
            item,
        )?;
        cache.add_data(DataType::from(Module::Unknown), &data)?;
    }

    let mut query = QueryBuilder::default();
    query.filter(Comparator::Gte, "port", 10001)?;
    let data_list = cache.get_data(DataType::from(Module::Unknown), Some(&query))?;
    for data in data_list {
        let item = data.decode::<Item>()?;
        println!("Item::port={}", item.port);
        println!("Item::data={}", item.data);
    }

    cache.empty(DataType::from(Module::Unknown))?;

    let count = cache.count(DataType::from(Module::Unknown), None)?;

    assert!(count == 0);
    Ok(())
}

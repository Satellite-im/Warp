use std::collections::HashMap;
use warp::error::Error;
use warp::libipld::Ipld;
use warp::pocket_dimension::query::{ComparatorFilter, QueryBuilder};
use warp::pocket_dimension::PocketDimension;
use warp::{
    data::DataType,
    module::Module,
    sata::{Sata, State},
    Extension, SingleHandle,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Default)]
pub struct MemoryClient {
    client: HashMap<DataType, Vec<Sata>>,
}

impl Extension for MemoryClient {
    fn id(&self) -> String {
        String::from("warp-pd-memory")
    }

    fn name(&self) -> String {
        String::from("In-Memory Caching System")
    }

    fn description(&self) -> String {
        String::from("")
    }

    fn module(&self) -> Module {
        Module::Cache
    }
}

impl MemoryClient {
    pub fn new() -> Self {
        let client = HashMap::new();
        Self { client }
    }
}

impl SingleHandle for MemoryClient {}

impl PocketDimension for MemoryClient {
    fn add_data(&mut self, dimension: DataType, data: &Sata) -> Result<()> {
        let mut data = data.clone();
        if let Some(value) = self.client.get_mut(&dimension) {
            let version = value.iter().filter(|item| item.id() == data.id()).count() as u32;
            data.set_version(version);
            value.push(data);
        } else {
            self.client.insert(dimension, vec![data]);
        }
        Ok(())
    }

    fn has_data(&mut self, dimension: DataType, query: &QueryBuilder) -> Result<()> {
        self.client
            .get(&dimension)
            .ok_or(Error::DataObjectNotFound)
            .and_then(|data| execute(data, query).map(|_| ()))
    }

    fn get_data(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<Vec<Sata>> {
        let data = self
            .client
            .get(&dimension)
            .ok_or(Error::DataObjectNotFound)?;

        match query {
            Some(query) => execute(data, query),
            None => Ok(data.to_vec()),
        }
    }

    fn size(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64> {
        self.get_data(dimension, query)
            .map(|data| data.iter().map(|i| i.data().len() as i64).sum())
    }

    fn count(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64> {
        self.get_data(dimension, query)
            .map(|data| data.len() as i64)
    }

    fn empty(&mut self, dimension: DataType) -> Result<()> {
        self.client
            .remove(&dimension)
            .ok_or(Error::DataObjectNotFound)
            .map(|_| ())
    }
}

// #[cfg(not(target_arch = "wasm32"))]
// fn get_payload_as_value(data: &Sata) -> Result<Ipld> {
//     data.decode().map_err(Error::from)
// }

pub(crate) fn execute(data: &[Sata], query: &QueryBuilder) -> Result<Vec<Sata>> {
    let mut list = Vec::new();
    for data in data.iter() {
        let object = match data.state() {
            State::Encoded => data.decode::<Ipld>()?,
            _ => continue,
        };

        match object {
            Ipld::Map(_) => {}
            _ => continue,
        };

        for (key, val) in query.get_where().iter() {
            if let Ok(result) = object.get(key.as_str()) {
                if *val == *result {
                    list.push(data.clone());
                }
            }
        }

        for comp in query.get_comparator().iter() {
            match comp {
                ComparatorFilter::Eq(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        if *result == *val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Ne(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        if *result != *val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Gte(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        match (result, val) {
                            (Ipld::Integer(res), Ipld::Integer(v)) if *res >= *v => {}
                            (Ipld::Float(res), Ipld::Float(v)) if *res >= *v => {}
                            _ => continue,
                        };
                        if list.contains(data) {
                            continue;
                        }
                        list.push(data.clone());
                    }
                }
                ComparatorFilter::Gt(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        match (result, val) {
                            (Ipld::Integer(res), Ipld::Integer(v)) if *res > *v => {}
                            (Ipld::Float(res), Ipld::Float(v)) if *res > *v => {}
                            _ => continue,
                        };
                        if list.contains(data) {
                            continue;
                        }
                        list.push(data.clone());
                    }
                }
                ComparatorFilter::Lte(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        match (result, val) {
                            (Ipld::Integer(res), Ipld::Integer(v)) if *res <= *v => {}
                            (Ipld::Float(res), Ipld::Float(v)) if *res <= *v => {}
                            _ => continue,
                        };
                        if list.contains(data) {
                            continue;
                        }
                        list.push(data.clone());
                    }
                }
                ComparatorFilter::Lt(key, val) => {
                    if let Ok(result) = object.get(key.as_str()) {
                        match (result, val) {
                            (Ipld::Integer(res), Ipld::Integer(v)) if *res < *v => {}
                            (Ipld::Float(res), Ipld::Float(v)) if *res < *v => {}
                            _ => continue,
                        };
                        if list.contains(data) {
                            continue;
                        }
                        list.push(data.clone());
                    }
                }
            }
        }

        if let Some(limit) = query.get_limit() {
            if list.len() > limit {
                list = list.drain(..limit).collect();
            }
        }
    }
    Ok(list)
}

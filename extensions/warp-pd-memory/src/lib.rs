use std::collections::HashMap;
use warp::{
    data::{DataObject, DataType},
    module::Module,
    Extension, SingleHandle,
};

use warp::error::Error;
use warp::pocket_dimension::query::{ComparatorFilter, QueryBuilder};
use warp::pocket_dimension::PocketDimension;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Default)]
pub struct MemoryClient {
    client: HashMap<DataType, Vec<DataObject>>,
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
    fn add_data(&mut self, dimension: DataType, data: &DataObject) -> Result<()> {
        let mut data = data.clone();
        data.set_data_type(dimension);

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

    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&QueryBuilder>,
    ) -> Result<Vec<DataObject>> {
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
            .map(|data| data.iter().map(|i| i.size() as i64).sum())
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

#[cfg(not(target_arch = "wasm32"))]
fn get_payload_as_value(data: &DataObject) -> Result<serde_json::Value> {
    data.payload().map_err(Error::from)
}

#[cfg(target_arch = "wasm32")]
fn get_payload_as_value(data: &DataObject) -> Result<serde_json::Value> {
    let jsvalue = data.payload()?;
    serde_wasm_bindgen::from_value(jsvalue)
        .map_err(|e| anyhow::anyhow!("{}", e))
        .map_err(Error::Any)
}

pub(crate) fn execute(data: &[DataObject], query: &QueryBuilder) -> Result<Vec<DataObject>> {
    let mut list = Vec::new();
    for data in data.iter() {
        let object = get_payload_as_value(data)?;

        if !object.is_object() {
            continue;
        }
        let object = object.as_object().ok_or(Error::Other)?;
        for (key, val) in query.get_where().iter() {
            if let Some(result) = object.get(key) {
                if val == result {
                    list.push(data.clone());
                }
            }
        }
        for comp in query.get_comparator().iter() {
            match comp {
                ComparatorFilter::Eq(key, val) => {
                    if let Some(result) = object.get(key) {
                        if result == val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Ne(key, val) => {
                    if let Some(result) = object.get(key) {
                        if result != val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Gte(key, val) => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result >= val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Gt(key, val) => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result > val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Lte(key, val) => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result <= val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Lt(key, val) => {
                    if let Some(result) = object.get(key) {
                        let result = result.as_i64().unwrap();
                        let val = val.as_i64().unwrap();
                        if result < val {
                            if list.contains(data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
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

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn pocketdimension_pd_memory() -> warp::pocket_dimension::PocketDimensionAdapter {
    let client = MemoryClient::new();
    warp::pocket_dimension::PocketDimensionAdapter::new(warp::sync::Arc::new(
        warp::sync::Mutex::new(Box::new(client)),
    ))
}

pub mod ffi {
    use crate::MemoryClient;
    use warp::pocket_dimension::PocketDimensionAdapter;
    use warp::sync::{Arc, Mutex};

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn pocketdimension_memory_new() -> *mut PocketDimensionAdapter {
        let obj = Box::new(PocketDimensionAdapter::new(Arc::new(Mutex::new(Box::new(
            MemoryClient::new(),
        )))));
        Box::into_raw(obj) as *mut PocketDimensionAdapter
    }
}

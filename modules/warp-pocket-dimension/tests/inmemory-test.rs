use std::collections::HashMap;
use warp_common::error::Error;
use warp_common::serde::{Deserialize, Serialize};
use warp_common::serde_json::Value;
use warp_common::{Extension, Result};
use warp_data::{DataObject, DataType};
use warp_module::Module;
use warp_pocket_dimension::query::{Comparator, ComparatorFilter, QueryBuilder};
use warp_pocket_dimension::PocketDimension;

// MemoryCache instance will hold a map of both module and dataobject "in memory".
// There is little functionality to it for testing purchase outside of `PocketDimension` interface
// Note: This `MemoryCache` is a cheap and dirty way of testing currently.
//      Such code here should not really be used in production
#[derive(Default)]
pub struct MemoryCache(HashMap<DataType, Vec<DataObject>>);

impl MemoryCache {
    pub fn flush(&mut self) {
        let _ = self.0.drain().collect::<Vec<_>>();
    }
}

impl Extension for MemoryCache {
    fn name(&self) -> String {
        "MemoryCache".to_string()
    }

    fn module(&self) -> Module {
        Module::Cache
    }
}

impl PocketDimension for MemoryCache {
    fn add_data(&mut self, dimension: DataType, data: &DataObject) -> Result<()> {
        //TODO: Determine size of payload for `DataObject::size`
        let mut object = data.clone();
        object.set_data_type(&dimension);
        if let Some(val) = self.0.get_mut(&dimension) {
            let objects = val
                .iter()
                .filter(|item| item.id == data.id)
                .collect::<Vec<&DataObject>>();

            if objects.contains(&data) {
                return Err(Error::DataObjectExist);
            }

            let version = objects.len() as u32;
            object.version = version;
            val.push(object);
        } else {
            self.0.insert(dimension, vec![object]);
        }
        Ok(())
    }

    fn has_data(&mut self, dimension: DataType, query: &QueryBuilder) -> Result<()> {
        let data = self.0.get(&dimension).ok_or(Error::Other)?;
        execute(data, query).map(|_| ())
    }

    fn get_data(
        &self,
        dimension: DataType,
        query: Option<&QueryBuilder>,
    ) -> Result<Vec<DataObject>> {
        let data = self.0.get(&dimension).ok_or(Error::Other)?;
        match query {
            Some(query) => execute(data, query),
            None => Ok(data.clone()),
        }
    }

    fn size(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64> {
        self.get_data(dimension, query)
            .map(|data| data.iter().map(|i| i.size as i64).sum())
    }

    fn count(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<i64> {
        self.get_data(dimension, query)
            .map(|data| data.len() as i64)
    }

    fn empty(&mut self, dimension: DataType) -> Result<()> {
        self.0.remove(&dimension);

        if self.get_data(dimension, None).is_ok() {
            return Err(Error::Other);
        }

        Ok(())
    }
}

//Cheap "filter"
fn execute(data: &Vec<DataObject>, query: &QueryBuilder) -> Result<Vec<DataObject>> {
    let mut list = Vec::new();
    for data in data.iter() {
        let object = data.payload::<Value>()?;
        if !object.is_object() {
            continue;
        }
        let object = object.as_object().ok_or(Error::Other)?;
        for (key, val) in query.r#where.iter() {
            if let Some(result) = object.get(key) {
                if val == result {
                    list.push(data.clone());
                }
            }
        }
        for comp in query.comparator.iter() {
            match comp {
                ComparatorFilter::Eq(key, val) => {
                    if let Some(result) = object.get(key) {
                        if result == val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                ComparatorFilter::Ne(key, val) => {
                    if let Some(result) = object.get(key) {
                        if result != val {
                            if list.contains(&data) {
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
                            if list.contains(&data) {
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
                            if list.contains(&data) {
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
                            if list.contains(&data) {
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
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
            }
        }

        if let Some(limit) = query.limit {
            if list.len() > limit {
                list = list.drain(..limit).collect();
            }
        }
    }
    Ok(list)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(crate = "warp_common::serde")]
pub struct SomeData {
    pub name: String,
    pub age: i64,
}

impl Default for SomeData {
    fn default() -> Self {
        Self {
            name: String::from("John Doe"),
            age: 21,
        }
    }
}

impl SomeData {
    pub fn set_name<S: AsRef<str>>(&mut self, name: S) {
        self.name = name.as_ref().to_string();
    }
    pub fn set_age(&mut self, age: i64) {
        self.age = age
    }
}

fn generate_data(system: &mut MemoryCache, amount: i64) {
    let mut object = DataObject::default();

    for i in 0..amount {
        let mut data = SomeData::default();
        data.set_name(&format!("Test Subject {i}"));
        data.set_age(18 + i);

        object.set_payload(data).unwrap();
        system
            .add_data(DataType::Module(Module::Accounts), &object)
            .unwrap();
    }
}

#[test]
fn if_count_eq_five() -> Result<()> {
    let mut memory = MemoryCache::default();

    generate_data(&mut memory, 100);

    let mut query = QueryBuilder::default();
    query.filter(Comparator::Gte, "age", 19)?.limit(5);

    let count = memory.count(DataType::Module(Module::Accounts), Some(&query))?;

    assert_eq!(count, 5);

    Ok(())
}

#[test]
fn data_test() -> Result<()> {
    let mut memory = MemoryCache::default();

    generate_data(&mut memory, 100);

    let mut query = QueryBuilder::default();
    query.r#where("age", 21)?;

    let data = memory.get_data(DataType::Module(Module::Accounts), Some(&query))?;

    assert_eq!(data.get(0).unwrap().payload::<SomeData>().unwrap().age, 21);

    Ok(())
}

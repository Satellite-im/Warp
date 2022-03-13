pub mod error;

use warp_data::DataObject;
use warp_module::Module;

use stretto::Cache;

use error::Error;
use warp_common::{serde_json, Extension};
use warp_pocket_dimension::query::{Comparator, QueryBuilder};
use warp_pocket_dimension::PocketDimension;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct StrettoClient {
    client: Cache<Module, Vec<DataObject>>,
}

impl Extension for StrettoClient {
    fn name(&self) -> String {
        String::from("warp-pd-stretto")
    }

    fn description(&self) -> String {
        format!("Pocket Dimension implementation with Stretto, a high performance thread-safe memory cache written in rust.")
    }

    fn module(&self) -> Module {
        Module::FileSystem
    }
}

impl StrettoClient {
    pub fn new() -> Result<Self> {
        let client = Cache::new(12960, 1e6 as i64)?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &Cache<Module, Vec<DataObject>> {
        &self.client
    }
}

impl PocketDimension for StrettoClient {
    fn add_data(
        &mut self,
        dimension: Module,
        data: &DataObject,
    ) -> std::result::Result<(), warp_common::error::Error> {
        let mut data = data.clone();
        data.set_module(&dimension);
        if data.module != dimension {
            return Err(warp_common::error::Error::Other);
        }
        if let Some(mut value) = self.client.get_mut(&dimension) {
            let objects = value
                .value()
                .iter()
                .filter(|item| item.id == data.id)
                .collect::<Vec<&DataObject>>();
            let version = objects.len() as u32;
            data.version = version;
            (*value.value_mut()).push(data);
            self.client
                .wait()
                .map_err(|_| warp_common::error::Error::Other)?;
        } else {
            self.client.insert(dimension, vec![data], 1);
            self.client
                .wait()
                .map_err(|_| warp_common::error::Error::Other)?;
        }
        Ok(())
    }

    fn has_data(&mut self, dimension: Module, query: &QueryBuilder) -> warp_common::Result<()> {
        let data = self
            .client
            .get(&dimension.into())
            .ok_or(warp_common::error::Error::Other)?;
        execute(data.value(), query).map(|_| ())
    }

    fn get_data(
        &self,
        dimension: Module,
        query: Option<&QueryBuilder>,
    ) -> std::result::Result<Vec<DataObject>, warp_common::error::Error> {
        let data = self
            .client
            .get(&dimension.into())
            .ok_or(warp_common::error::Error::Other)?;

        let data = data.value();
        match query {
            Some(query) => execute(data, query),
            None => Ok(data.clone()),
        }
    }

    fn size(
        &self,
        dimension: Module,
        query: Option<&QueryBuilder>,
    ) -> std::result::Result<i64, warp_common::error::Error> {
        self.get_data(dimension, query)
            .map(|data| data.iter().map(|i| i.size as i64).sum())
    }

    fn count(
        &self,
        dimension: Module,
        query: Option<&QueryBuilder>,
    ) -> std::result::Result<i64, warp_common::error::Error> {
        self.get_data(dimension, query)
            .map(|data| data.len() as i64)
    }

    fn empty(&mut self, _: Module) -> std::result::Result<(), warp_common::error::Error> {
        // Note, since stretto doesnt clear base on key, we will clear everything when this is
        // call for now.
        // TODO: Implement a direct clear or mutation for the dimension

        self.client
            .clear()
            .map_err(|_| warp_common::error::Error::Other)?;

        self.client
            .wait()
            .map_err(|_| warp_common::error::Error::Other)
    }
}

pub(crate) fn execute(
    data: &Vec<DataObject>,
    query: &QueryBuilder,
) -> std::result::Result<Vec<DataObject>, warp_common::error::Error> {
    let mut list = Vec::new();
    for data in data.iter() {
        let object = data.payload::<serde_json::Value>()?;
        if !object.is_object() {
            continue;
        }
        let object = object.as_object().ok_or(warp_common::error::Error::Other)?;
        for (key, val) in query.r#where.iter() {
            if let Some(result) = object.get(key) {
                if val == result {
                    list.push(data.clone());
                }
            }
        }
        for (comp, key, val) in query.comparator.iter() {
            match comp {
                Comparator::Eq => {
                    if let Some(result) = object.get(key) {
                        if result == val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Ne => {
                    if let Some(result) = object.get(key) {
                        if result != val {
                            if list.contains(&data) {
                                continue;
                            }
                            list.push(data.clone());
                        }
                    }
                }
                Comparator::Gte => {
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
                Comparator::Gt => {
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
                Comparator::Lte => {
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
                Comparator::Lt => {
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

#[cfg(test)]
mod test {
    use crate::StrettoClient;
    use warp_common::error::Error;
    use warp_common::serde::{Deserialize, Serialize};
    use warp_data::DataObject;
    use warp_module::Module;
    use warp_pocket_dimension::query::{Comparator, QueryBuilder};
    use warp_pocket_dimension::PocketDimension;

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

    fn generate_data(system: &mut StrettoClient, amount: i64) {
        let mut object = DataObject::default();
        object.module = Module::Accounts;

        for i in 0..amount {
            let mut data = SomeData::default();
            data.set_name(&format!("Test Subject {i}"));
            data.set_age(18 + i);

            object.set_payload(data).unwrap();
            system.add_data(Module::Accounts, &object).unwrap();
        }
    }

    #[test]
    fn if_count_eq_five() -> Result<(), Error> {
        let mut memory = StrettoClient::new().map_err(|_| Error::Other)?;

        generate_data(&mut memory, 100);

        let mut query = QueryBuilder::default();
        query.filter(Comparator::Gte, "age", 19)?.limit(5);

        let count = memory.count(Module::Accounts, Some(&query))?;

        assert_eq!(count, 5);

        Ok(())
    }
}

use anyhow::anyhow;
use warp::{
    data::DataType,
    libipld::Ipld,
    module::Module,
    sata::{Sata, State},
    Extension, SingleHandle,
};

use stretto::Cache;

use warp::error::Error;

use warp::pocket_dimension::query::{ComparatorFilter, QueryBuilder};
use warp::pocket_dimension::PocketDimension;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone)]
pub struct StrettoClient {
    client: Cache<DataType, Vec<Sata>>,
}

impl Extension for StrettoClient {
    fn id(&self) -> String {
        String::from("warp-pd-stretto")
    }

    fn name(&self) -> String {
        String::from("Stretto Caching System")
    }

    fn description(&self) -> String {
        String::from("Pocket Dimension implementation with Stretto, a high performance thread-safe memory cache written in rust.")
    }

    fn module(&self) -> Module {
        Module::Cache
    }
}

impl StrettoClient {
    pub fn new() -> anyhow::Result<Self> {
        let client = Cache::new(12960, 1e6 as i64)?;
        Ok(Self { client })
    }

    pub fn client(&self) -> &Cache<DataType, Vec<Sata>> {
        &self.client
    }
}

impl SingleHandle for StrettoClient {}

impl PocketDimension for StrettoClient {
    fn add_data(&mut self, dimension: DataType, data: &Sata) -> Result<()> {
        let data = data.clone();
        // data.set_data_type(dimension);

        if let Some(mut value) = self.client.get_mut(&dimension) {
            // let version = value
            //     .value()
            //     .iter()
            //     .filter(|item| item.id() == data.id())
            //     .count() as u32;
            // data.set_version(version);
            (*value.value_mut()).push(data);
            self.client
                .wait()
                .map_err(|_| warp::error::Error::DataObjectNotFound)?;
        } else {
            self.client.insert(dimension, vec![data], 1);
            self.client
                .wait()
                .map_err(|_| warp::error::Error::DataObjectNotFound)?;
        }
        Ok(())
    }

    fn has_data(&mut self, dimension: DataType, query: &QueryBuilder) -> Result<()> {
        self.client
            .get(&dimension)
            .ok_or(warp::error::Error::DataObjectNotFound)
            .and_then(|data| execute(data.value(), query).map(|_| ()))
    }

    fn get_data(&self, dimension: DataType, query: Option<&QueryBuilder>) -> Result<Vec<Sata>> {
        let data = self
            .client
            .get(&dimension)
            .ok_or(warp::error::Error::DataObjectNotFound)?;

        let data = data.value();
        match query {
            Some(query) => execute(data, query),
            None => Ok(data.clone()),
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
        if let Some(mut dim) = self.client.get_mut(&dimension) {
            dim.value_mut().clear();
        }

        self.client
            .wait()
            .map_err(|e| anyhow!(e))
            .map_err(Error::from)
    }
}

//TODO: Rewrite
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

// pub mod ffi {
//     use crate::StrettoClient;
//     use warp::pocket_dimension::PocketDimensionAdapter;
//     use warp::sync::{Arc, RwLock};

//     #[allow(clippy::missing_safety_doc)]
//     #[no_mangle]
//     pub unsafe extern "C" fn pocketdimension_stretto_new() -> *mut PocketDimensionAdapter {
//         let client = match StrettoClient::new() {
//             Ok(client) => PocketDimensionAdapter::new(Arc::new(RwLock::new(Box::new(client)))),
//             Err(_) => return std::ptr::null_mut(),
//         };

//         let obj = Box::new(client);
//         Box::into_raw(obj) as *mut PocketDimensionAdapter
//     }
// }

#[cfg(test)]
mod test {
    use crate::StrettoClient;
    use serde::{Deserialize, Serialize};
    use warp::data::DataType;
    use warp::error::Error;
    use warp::module::Module;
    use warp::pocket_dimension::query::{Comparator, QueryBuilder};
    use warp::pocket_dimension::PocketDimension;
    use warp::sata::Sata;

    #[derive(Serialize, Deserialize, Debug, Clone)]
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
        for i in 0..amount {
            let mut data = SomeData::default();
            data.set_name(&format!("Test Subject {i}"));
            data.set_age(18 + i);

            let object = Sata::default()
                .encode(
                    warp::libipld::IpldCodec::DagJson,
                    warp::sata::Kind::Reference,
                    data,
                )
                .unwrap();
            system
                .add_data(DataType::from(Module::Accounts), &object)
                .unwrap();
        }
    }

    #[test]
    fn if_count_eq_five() -> Result<(), Error> {
        let mut memory = StrettoClient::new().map_err(|_| Error::Other)?;

        generate_data(&mut memory, 100);

        let mut query = QueryBuilder::default();
        query.filter(Comparator::Gte, "age", 19)?.limit(5);

        let count = memory.count(DataType::from(Module::Accounts), Some(&query))?;

        assert_eq!(count, 5);

        Ok(())
    }
}

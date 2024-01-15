use libipld::{serde::to_ipld, Ipld};

use crate::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::{self};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(C)]
#[serde(rename_all = "lowercase")]
pub enum Comparator {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
    Ne,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ComparatorFilter {
    Eq(String, Ipld),
    Gt(String, Ipld),
    Gte(String, Ipld),
    Lt(String, Ipld),
    Lte(String, Ipld),
    Ne(String, Ipld),
}

impl From<(Comparator, String, Ipld)> for ComparatorFilter {
    fn from((comp, key, val): (Comparator, String, Ipld)) -> Self {
        match comp {
            Comparator::Eq => ComparatorFilter::Eq(key, val),
            Comparator::Ne => ComparatorFilter::Ne(key, val),
            Comparator::Gt => ComparatorFilter::Gt(key, val),
            Comparator::Gte => ComparatorFilter::Gte(key, val),
            Comparator::Lt => ComparatorFilter::Lt(key, val),
            Comparator::Lte => ComparatorFilter::Lte(key, val),
        }
    }
}

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct QueryBuilder {
    r#where: Vec<(String, Ipld)>,
    comparator: Vec<ComparatorFilter>,
    limit: Option<usize>,
}

impl QueryBuilder {
    pub fn import(data: &str) -> Result<QueryBuilder, Error> {
        serde_json::from_str(data).map_err(Error::SerdeJsonError)
    }
}

impl QueryBuilder {
    pub fn get_where(&self) -> Vec<(String, Ipld)> {
        self.r#where.clone()
    }

    pub fn get_comparator(&self) -> Vec<ComparatorFilter> {
        self.comparator.clone()
    }

    pub fn get_limit(&self) -> Option<usize> {
        self.limit
    }
}

impl QueryBuilder {
    pub fn r#where<I>(&mut self, key: &str, value: I) -> Result<&mut Self, Error>
    where
        I: Serialize,
    {
        self.r#where.push((
            key.to_string(),
            to_ipld(value).map_err(anyhow::Error::from)?,
        ));
        Ok(self)
    }

    pub fn filter<I>(
        &mut self,
        comparator: Comparator,
        key: &str,
        value: I,
    ) -> Result<&mut Self, Error>
    where
        I: Serialize,
    {
        self.comparator.push(ComparatorFilter::from((
            comparator,
            key.to_string(),
            to_ipld(value).map_err(anyhow::Error::from)?,
        )));
        Ok(self)
    }

    pub fn limit(&mut self, limit: usize) -> &mut Self {
        self.limit = Some(limit);
        self
    }
}

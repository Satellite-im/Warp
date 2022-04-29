use super::Result;
use serde::Serialize;
use serde_json::{self, Value};

#[derive(Debug)]
pub enum Comparator {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
    Ne,
}

#[derive(Debug)]
pub enum ComparatorFilter {
    Eq(String, Value),
    Gt(String, Value),
    Gte(String, Value),
    Lt(String, Value),
    Lte(String, Value),
    Ne(String, Value),
}

impl From<(Comparator, String, Value)> for ComparatorFilter {
    fn from((comp, key, val): (Comparator, String, Value)) -> Self {
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

#[derive(Default, Debug)]
pub struct QueryBuilder {
    pub r#where: Vec<(String, Value)>,
    pub comparator: Vec<ComparatorFilter>,
    pub limit: Option<usize>,
}

impl QueryBuilder {
    pub fn r#where<S, I>(&mut self, key: S, value: I) -> Result<&mut Self>
    where
        S: AsRef<str>,
        I: Serialize,
    {
        self.r#where
            .push((key.as_ref().to_string(), serde_json::to_value(value)?));
        Ok(self)
    }

    pub fn filter<I>(&mut self, comparator: Comparator, key: &str, value: I) -> Result<&mut Self>
    where
        I: Serialize,
    {
        self.comparator.push(ComparatorFilter::from((
            comparator,
            key.to_string(),
            serde_json::to_value(value)?,
        )));
        Ok(self)
    }

    pub fn limit(&mut self, limit: usize) -> &mut Self {
        self.limit = Some(limit);
        self
    }
}

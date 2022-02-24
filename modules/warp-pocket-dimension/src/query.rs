use warp_common::serde::Serialize;
use warp_common::serde_json::{self, Value};
use warp_common::{error::Error, Result};

#[derive(Debug)]
pub enum Comparator {
    Eq,
    Gt,
    Gte,
    Lt,
    Lte,
    Ne,
}

#[derive(Default, Debug)]
pub struct QueryBuilder {
    pub r#where: Vec<(String, Value)>,
    pub comparator: Vec<(Comparator, String, Value)>,
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

    pub fn filter<S, I>(&mut self, compatator: Comparator, key: S, value: I) -> Result<&mut Self>
    where
        S: AsRef<str>,
        I: Serialize,
    {
        let value = serde_json::to_value(value)?;
        match compatator {
            Comparator::Eq | Comparator::Ne => {
                self.comparator
                    .push((compatator, key.as_ref().to_owned(), value))
            }
            Comparator::Gt | Comparator::Gte | Comparator::Lt | Comparator::Lte => {
                //Due it being greater or less than (or similar), we need to check that `value` is numeric
                if !value.is_number() {
                    return Err(Error::Other); //Mismatch
                }
                self.comparator
                    .push((compatator, key.as_ref().to_owned(), value))
            }
        };
        Ok(self)
    }

    pub fn limit(&mut self, limit: usize) -> &mut Self {
        self.limit = Some(limit);
        self
    }
}

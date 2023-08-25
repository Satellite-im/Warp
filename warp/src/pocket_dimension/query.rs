use libipld::{serde::to_ipld, Ipld};

use crate::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::{self};
use warp_derive::FFIFree;

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

#[derive(Default, Serialize, Deserialize, Debug, Clone, FFIFree)]
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

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::error::Error;
    use crate::ffi::FFIResult;
    use crate::pocket_dimension::query::{Comparator, QueryBuilder};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn querybuilder_new() -> *mut QueryBuilder {
        Box::into_raw(Box::default())
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn querybuilder_import(data: *const c_char) -> FFIResult<QueryBuilder> {
        if data.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Query cannot be null")));
        }
        let data = CStr::from_ptr(data).to_string_lossy().to_string();

        match serde_json::from_str(&data).map_err(Error::from) {
            Ok(q) => FFIResult::ok(q),
            Err(e) => FFIResult::err(e),
        }
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn querybuilder_where(
        ctx: *mut QueryBuilder,
        key: *const c_char,
        val: *const c_char,
    ) {
        if ctx.is_null() {
            return;
        }

        if key.is_null() {
            return;
        }

        if val.is_null() {
            return;
        }

        let query = &mut *ctx;
        let key = CStr::from_ptr(key).to_string_lossy().to_string();
        let val = CStr::from_ptr(val).to_string_lossy().to_string();

        if query.r#where(&key, val).is_ok() {}
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn querybuilder_filter(
        ctx: *mut QueryBuilder,
        cmp: Comparator,
        key: *const c_char,
        val: *const c_char,
    ) {
        if ctx.is_null() {
            return;
        }

        if key.is_null() {
            return;
        }

        if val.is_null() {
            return;
        }

        let query = &mut *ctx;
        let key = CStr::from_ptr(key).to_string_lossy().to_string();
        let val = CStr::from_ptr(val).to_string_lossy().to_string();

        if query.filter(cmp, &key, val).is_ok() {}
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn querybuilder_limit(ctx: *mut QueryBuilder, limit: usize) {
        if ctx.is_null() {
            return;
        }

        let query = &mut *ctx;

        query.limit(limit);
    }
}

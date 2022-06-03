#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use super::Result;
use crate::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use warp_derive::FFIFree;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[repr(C)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
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

#[derive(Default, Serialize, Deserialize, Debug, Clone, FFIFree)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct QueryBuilder {
    r#where: Vec<(String, Value)>,
    comparator: Vec<ComparatorFilter>,
    limit: Option<usize>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl QueryBuilder {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
    pub fn import(data: &str) -> Result<QueryBuilder> {
        serde_json::from_str(data).map_err(Error::SerdeJsonError)
    }
}

impl QueryBuilder {
    pub fn get_where(&self) -> Vec<(String, Value)> {
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
    pub fn r#where<I>(&mut self, key: &str, value: I) -> Result<&mut Self>
    where
        I: Serialize,
    {
        self.r#where
            .push((key.to_string(), serde_json::to_value(value)?));
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
        Box::into_raw(Box::new(QueryBuilder::default())) as *mut QueryBuilder
    }

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn querybuilder_import(data: *const c_char) -> FFIResult<QueryBuilder> {
        if data.is_null() {
            return FFIResult::err(Error::Any(anyhow::anyhow!("Query is null")));
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

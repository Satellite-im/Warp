use super::Result;
use serde::Serialize;
use serde_json::{self, Value};

#[derive(Debug)]
#[repr(C)]
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
    use crate::pocket_dimension::query::{Comparator, QueryBuilder};
    use libc::c_char;
    use std::ffi::CStr;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn querybuilder_new() -> *mut QueryBuilder {
        Box::into_raw(Box::new(QueryBuilder::default())) as *mut QueryBuilder
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

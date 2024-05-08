use futures::{stream::BoxStream, StreamExt};
use js_sys::Promise;
use wasm_bindgen::prelude::*;

/// Wraps BoxStream<'static, TesseractEvent> into a js compatible struct
#[wasm_bindgen]
pub struct AsyncIterator {
    inner: BoxStream<'static, JsValue>,
}
impl AsyncIterator {
    pub fn new(stream: BoxStream<'static, JsValue>) -> Self {
        Self { inner: stream }
    }
}

/// Provides the next() function expected by js async iterator
#[wasm_bindgen]
impl AsyncIterator {
    pub async fn next(&mut self) -> std::result::Result<Promise, JsError> {
        let next = self.inner.next().await;
        match next {
            Some(value) => Ok(Promise::resolve(&PromiseResult::new(value.into()).into())),
            None => std::result::Result::Err(JsError::new("returned None")),
        }
    }
}

/// Wraps in the TesseractEvent promise result in the js object expected by js async iterator
#[wasm_bindgen]
struct PromiseResult {
    value: JsValue,
    pub done: bool,
}

#[wasm_bindgen]
impl PromiseResult {
    pub fn new(value: JsValue) -> Self {
        Self { value, done: false }
    }

    #[wasm_bindgen(getter)]
    pub fn value(&self) -> JsValue {
        self.value.clone()
    }
}

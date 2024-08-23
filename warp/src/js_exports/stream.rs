use futures::{stream::BoxStream, Stream, StreamExt};
use js_sys::Promise;
use std::pin::Pin;
use std::task::{Context, Poll};
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

pub struct InnerStream {
    inner: send_wrapper::SendWrapper<wasm_streams::readable::IntoStream<'static>>,
}

impl From<wasm_streams::ReadableStream> for InnerStream {
    fn from(stream: wasm_streams::ReadableStream) -> Self {
        Self {
            inner: send_wrapper::SendWrapper::new(stream.into_stream()),
        }
    }
}

impl Stream for InnerStream {
    type Item = std::io::Result<bytes::Bytes>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        match futures::ready!(Pin::new(&mut this.inner).poll_next(cx)) {
            Some(Ok(val)) => {
                let bytes = serde_wasm_bindgen::from_value(val).expect("valid bytes");
                return Poll::Ready(Some(Ok(bytes)));
            }
            Some(Err(e)) => {
                //TODO: Make inner stream optional and take from stream to prevent repeated polling
                let str: String = serde_wasm_bindgen::from_value(e).expect("valid string");
                return Poll::Ready(Some(Err(std::io::Error::other(str))));
            }
            _ => {
                // Any other condition should cause this stream to end
                return Poll::Ready(None);
            }
        }
    }
}

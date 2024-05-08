use crate::constellation::Constellation;
use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen]
pub struct ConstellationBox {
    inner: Box<dyn Constellation>,
}
impl ConstellationBox {
    pub fn new(constellation: Box<dyn Constellation>) -> Self {
        Self {
            inner: constellation,
        }
    }
}

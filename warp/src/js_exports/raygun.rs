use crate::raygun::RayGun;
use wasm_bindgen::prelude::*;

#[derive(Clone)]
#[wasm_bindgen]
pub struct RayGunBox {
    inner: Box<dyn RayGun>,
}
impl RayGunBox {
    pub fn new(raygun: Box<dyn RayGun>) -> Self {
        Self { inner: raygun }
    }
}

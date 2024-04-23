use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn function_example(arg: &str) -> String {
    web_sys::console::log_1(&format!("WASM: arg: {}", arg).into());
    web_sys::console::log_1(&"WASM: returning: bye".into());
    "bye".to_string()
}

#[wasm_bindgen]
pub struct StructExample {
    internal: String,
}

#[wasm_bindgen]
impl StructExample {
    #[wasm_bindgen(constructor)]
    pub fn new(val: String) -> StructExample {
        StructExample { internal: val }
    }

    pub fn get(&self) -> String {
        self.internal.clone()
    }

    pub fn set(&mut self, val: String) {
        self.internal = val;
    }
}

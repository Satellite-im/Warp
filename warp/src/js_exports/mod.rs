use crate::constellation::Constellation;
use crate::multipass::MultiPass;
use crate::raygun::RayGun;

use constellation::ConstellationBox;
use multipass::MultiPassBox;
use raygun::RayGunBox;

use wasm_bindgen::prelude::*;

pub mod constellation;
pub mod multipass;
pub mod raygun;
pub mod stream;

#[wasm_bindgen(start)]
pub fn initialize() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    tracing_wasm::set_as_global_default();
}

#[wasm_bindgen]
pub struct WarpInstance {
    multipass: MultiPassBox,
    raygun: RayGunBox,
    constellation: ConstellationBox,
}

impl WarpInstance {
    pub fn new(mp: Box<dyn MultiPass>, rg: Box<dyn RayGun>, fs: Box<dyn Constellation>) -> Self {
        let multipass = MultiPassBox::new(mp);
        let raygun = RayGunBox::new(rg);
        let constellation = ConstellationBox::new(fs);

        Self {
            multipass,
            raygun,
            constellation,
        }
    }
}
#[wasm_bindgen]
impl WarpInstance {
    #[wasm_bindgen(getter)]
    pub fn multipass(&self) -> MultiPassBox {
        self.multipass.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn raygun(&self) -> RayGunBox {
        self.raygun.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn constellation(&self) -> ConstellationBox {
        self.constellation.clone()
    }
}

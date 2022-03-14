use warp_common::{anyhow, Extension};

use std::sync::{Arc, Mutex};
use warp::{Constellation, MultiPass, PocketDimension, RayGun};

#[derive(Clone, Default)]
pub struct ModuleManager {
    pub filesystem: Option<Arc<Mutex<Box<dyn Constellation>>>>,
    pub cache: Option<Arc<Mutex<Box<dyn PocketDimension>>>>,
    pub account: Option<Arc<Mutex<Box<dyn MultiPass>>>>,
    pub messaging: Option<Arc<Mutex<Box<dyn RayGun>>>>,
}

impl ModuleManager {
    pub fn set_filesystem<T: Constellation + Extension + 'static>(&mut self, handle: T) {
        self.filesystem = Some(Arc::new(Mutex::new(Box::new(handle))));
    }

    pub fn set_cache<T: PocketDimension + Extension + 'static>(&mut self, handle: T) {
        self.cache = Some(Arc::new(Mutex::new(Box::new(handle))));
    }

    pub fn get_filesystem(&self) -> anyhow::Result<&Arc<Mutex<Box<dyn Constellation>>>> {
        Ok(self
            .filesystem
            .as_ref()
            .ok_or(warp_common::error::Error::ToBeDetermined)?)
    }

    pub fn get_cache(&self) -> anyhow::Result<&Arc<Mutex<Box<dyn PocketDimension>>>> {
        Ok(self
            .cache
            .as_ref()
            .ok_or(warp_common::error::Error::ToBeDetermined)?)
    }
}

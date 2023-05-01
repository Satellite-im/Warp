pub mod item;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use item::Item;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use warp::error::Error;

use warp::pocket_dimension::PocketDimension;
use warp::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use warp::constellation::directory::Directory;
use warp::hooks::Hooks;
use warp::module::Module;
use warp::{Extension, SingleHandle};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct MemorySystemInternal(item::directory::Directory);

impl AsRef<item::directory::Directory> for MemorySystemInternal {
    fn as_ref(&self) -> &item::directory::Directory {
        &self.0
    }
}

impl AsMut<item::directory::Directory> for MemorySystemInternal {
    fn as_mut(&mut self) -> &mut item::directory::Directory {
        &mut self.0
    }
}

impl Default for MemorySystemInternal {
    fn default() -> Self {
        MemorySystemInternal(item::directory::Directory::new("root"))
    }
}
#[derive(Serialize, Deserialize, Clone)]
pub struct MemorySystem {
    index: Directory,
    current: Directory,
    path: PathBuf,
    modified: DateTime<Utc>,
    #[serde(skip)]
    internal: Arc<RwLock<MemorySystemInternal>>,
    #[serde(skip)]
    cache: Option<Arc<RwLock<Box<dyn PocketDimension>>>>,
    #[serde(skip)]
    hooks: Option<Hooks>,
}

impl SingleHandle for MemorySystem {}

impl Default for MemorySystem {
    fn default() -> Self {
        Self {
            index: Directory::new("root"),
            current: Directory::new("root"),
            path: PathBuf::new(),
            modified: Utc::now(),
            internal: Default::default(),
            cache: None,
            hooks: None,
        }
    }
}

impl MemorySystem {
    pub fn new() -> Self {
        MemorySystem::default()
    }

    pub fn set_cache(&mut self, cache: Arc<RwLock<Box<dyn PocketDimension>>>) {
        self.cache = Some(cache);
    }

    pub fn set_hook(&mut self, hook: &Hooks) {
        self.hooks = Some(hook.clone())
    }

    pub fn get_cache(&self) -> anyhow::Result<RwLockReadGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = cache.read();
        Ok(inner)
    }

    pub fn get_cache_mut(&self) -> anyhow::Result<RwLockWriteGuard<Box<dyn PocketDimension>>> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Pocket Dimension Extension is not set"))?;

        let inner = cache.write();
        Ok(inner)
    }
}

impl MemorySystemInternal {
    pub fn new() -> Self {
        MemorySystemInternal::default()
    }
}

impl Extension for MemorySystem {
    fn id(&self) -> String {
        String::from("warp-fs-memory")
    }
    fn name(&self) -> String {
        String::from("Basic In-Memory FileSystem")
    }

    fn description(&self) -> String {
        String::from("Basic In-Memory Filesystem extension")
    }
    fn module(&self) -> Module {
        Module::FileSystem
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod ffi {
    use crate::MemorySystem;
    use warp::constellation::ConstellationAdapter;

    #[allow(clippy::missing_safety_doc)]
    #[no_mangle]
    pub unsafe extern "C" fn constellation_fs_memory_create_context() -> *mut ConstellationAdapter {
        let obj = Box::new(ConstellationAdapter::new(Box::new(MemorySystem::new())));
        Box::into_raw(obj) as *mut ConstellationAdapter
    }
}

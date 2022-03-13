use warp_common::{anyhow, Extension};
use warp_module::Module;

use warp::{Constellation, MultiPass, PocketDimension, RayGun};
use std::sync::{Arc, Mutex};

pub trait ModuleRegister<T> {
    fn register(inner: Box<T>) -> Self;
}

#[derive(Clone)]
pub struct FileSystem {
    pub name: String,
    pub handle: Arc<Mutex<Box<dyn Constellation>>>,
    pub active: bool,
}

impl<T> ModuleRegister<T> for FileSystem
where
    T: Constellation + Extension + 'static,
{
    fn register(inner: Box<T>) -> Self {
        Self {
            name: inner.name(),
            handle: Arc::new(Mutex::new(inner)),
            active: false,
        }
    }
}

#[derive(Clone)]
pub struct Cache {
    pub name: String,
    pub handle: Arc<Mutex<Box<dyn PocketDimension>>>,
    pub active: bool,
}

impl<T> ModuleRegister<T> for Cache
where
    T: PocketDimension + Extension + 'static,
{
    fn register(inner: Box<T>) -> Self {
        Self {
            name: inner.name(),
            handle: Arc::new(Mutex::new(inner)),
            active: false,
        }
    }
}

#[derive(Clone)]
pub struct Account {
    pub name: String,
    pub handle: Arc<Mutex<Box<dyn MultiPass>>>,
    pub active: bool,
}

impl<T> ModuleRegister<T> for Account
where
    T: MultiPass + Extension + 'static,
{
    fn register(inner: Box<T>) -> Self {
        Self {
            name: inner.name(),
            handle: Arc::new(Mutex::new(inner)),
            active: false,
        }
    }
}

#[derive(Clone)]
pub struct Messaging {
    pub name: String,
    pub handle: Arc<Mutex<Box<dyn RayGun>>>,
    pub active: bool,
}

impl<T> ModuleRegister<T> for Messaging
where
    T: RayGun + Extension + 'static,
{
    fn register(inner: Box<T>) -> Self {
        Self {
            name: inner.name(),
            handle: Arc::new(Mutex::new(inner)),
            active: false,
        }
    }
}

#[derive(Clone, Default)]
pub struct ModuleManager {
    pub filesystem: Vec<FileSystem>,
    pub cache: Vec<Cache>,
    pub account: Vec<Account>,
    pub messaging: Vec<Messaging>,
}

impl ModuleManager {
    pub fn register_filesystem<T: Constellation + Extension + 'static>(&mut self, handle: T) {
        for item in self.filesystem.iter() {
            if item.name.eq(&handle.name()) {
                return;
            }
        }
        self.filesystem.push(FileSystem::register(Box::new(handle)));
    }

    pub fn register_cache<T: PocketDimension + Extension + 'static>(&mut self, handle: T) {
        for item in self.cache.iter() {
            if item.name.eq(&handle.name()) {
                return;
            }
        }
        self.cache.push(Cache::register(Box::new(handle)));
    }

    pub fn register_account<T: MultiPass + Extension + 'static>(&mut self, handle: T) {
        for item in self.account.iter() {
            if item.name.eq(&handle.name()) {
                return;
            }
        }
        self.account.push(Account::register(Box::new(handle)));
    }

    pub fn register_messaging<T: RayGun + Extension + 'static>(&mut self, handle: T) {
        for item in self.messaging.iter() {
            if item.name.eq(&handle.name()) {
                return;
            }
        }
        self.messaging.push(Messaging::register(Box::new(handle)));
    }
}

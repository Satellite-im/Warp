use warp::error::Error;
use warp::{
    constellation::Constellation,
    multipass::MultiPass,
    pocket_dimension::PocketDimension,
    raygun::RayGun,
    sync::{Arc, RwLock},
};

pub trait Information {
    fn name(&self) -> String;
    fn id(&self) -> String;
}

#[derive(Clone)]
pub struct FileSystem {
    pub handle: Arc<RwLock<Box<dyn Constellation>>>,
    pub active: bool,
}

impl Information for FileSystem {
    fn name(&self) -> String {
        self.handle.read().name()
    }
    fn id(&self) -> String {
        self.handle.read().id()
    }
}

impl AsRef<Arc<RwLock<Box<dyn Constellation>>>> for FileSystem {
    fn as_ref(&self) -> &Arc<RwLock<Box<dyn Constellation>>> {
        &self.handle
    }
}

#[derive(Clone)]
pub struct Cache {
    pub handle: Arc<RwLock<Box<dyn PocketDimension>>>,
    pub active: bool,
}

impl AsRef<Arc<RwLock<Box<dyn PocketDimension>>>> for Cache {
    fn as_ref(&self) -> &Arc<RwLock<Box<dyn PocketDimension>>> {
        &self.handle
    }
}

impl Information for Cache {
    fn name(&self) -> String {
        self.handle.read().name()
    }
    fn id(&self) -> String {
        self.handle.read().id()
    }
}

#[derive(Clone)]
pub struct Account {
    pub handle: Arc<RwLock<Box<dyn MultiPass>>>,
    pub active: bool,
}

impl AsRef<Arc<RwLock<Box<dyn MultiPass>>>> for Account {
    fn as_ref(&self) -> &Arc<RwLock<Box<dyn MultiPass>>> {
        &self.handle
    }
}

impl Information for Messaging {
    fn name(&self) -> String {
        self.handle.read().name()
    }
    fn id(&self) -> String {
        self.handle.read().id()
    }
}

#[derive(Clone)]
pub struct Messaging {
    pub handle: Arc<RwLock<Box<dyn RayGun>>>,
    pub active: bool,
}

impl AsRef<Arc<RwLock<Box<dyn RayGun>>>> for Messaging {
    fn as_ref(&self) -> &Arc<RwLock<Box<dyn RayGun>>> {
        &self.handle
    }
}

impl Information for Account {
    fn name(&self) -> String {
        self.handle.read().name()
    }
    fn id(&self) -> String {
        self.handle.read().id()
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
    pub fn set_filesystem(&mut self, handle: Arc<RwLock<Box<dyn Constellation>>>) {
        if self
            .filesystem
            .iter()
            .filter(|fs| fs.id() == handle.read().id())
            .count()
            != 0
        {
            return;
        }
        self.filesystem.push(FileSystem {
            handle,
            active: false,
        });
    }

    pub fn enable_filesystem<S: AsRef<str>>(&mut self, id: S) -> anyhow::Result<()> {
        let id = id.as_ref();

        if self.filesystem.iter().filter(|item| item.active).count() >= 1 {
            let index = self
                .filesystem
                .iter()
                .position(|item| item.active)
                .ok_or(Error::ArrayPositionNotFound)?;

            self.filesystem
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?
                .active = false;
        }

        let index = self
            .filesystem
            .iter()
            .position(|item| item.id() == id)
            .ok_or(Error::ArrayPositionNotFound)?;

        self.filesystem
            .get_mut(index)
            .ok_or(Error::ArrayPositionNotFound)?
            .active = true;
        Ok(())
    }

    pub fn enable_cache<S: AsRef<str>>(&mut self, id: S) -> anyhow::Result<()> {
        let id = id.as_ref();

        if self.cache.iter().filter(|item| item.active).count() >= 1 {
            let index = self
                .cache
                .iter()
                .position(|item| item.active)
                .ok_or(Error::ArrayPositionNotFound)?;

            self.cache
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?
                .active = false;
        }

        let index = self
            .cache
            .iter()
            .position(|item| item.id() == id)
            .ok_or(Error::ArrayPositionNotFound)?;

        self.cache
            .get_mut(index)
            .ok_or(Error::ArrayPositionNotFound)?
            .active = true;

        Ok(())
    }

    pub fn set_cache(&mut self, handle: Arc<RwLock<Box<dyn PocketDimension>>>) {
        if self
            .cache
            .iter()
            .filter(|cs| cs.id() == handle.read().id())
            .count()
            != 0
        {
            return;
        }
        self.cache.push(Cache {
            handle,
            active: false,
        })
    }

    pub fn enable_account<S: AsRef<str>>(&mut self, id: S) -> anyhow::Result<()> {
        let id = id.as_ref();

        if self.account.iter().filter(|item| item.active).count() >= 1 {
            let index = self
                .account
                .iter()
                .position(|item| item.active)
                .ok_or(Error::ArrayPositionNotFound)?;

            self.account
                .get_mut(index)
                .ok_or(Error::ArrayPositionNotFound)?
                .active = false;
        }

        let index = self
            .account
            .iter()
            .position(|item| item.id() == id)
            .ok_or(Error::ArrayPositionNotFound)?;

        self.account
            .get_mut(index)
            .ok_or(Error::ArrayPositionNotFound)?
            .active = true;

        Ok(())
    }

    pub fn set_account(&mut self, handle: Arc<RwLock<Box<dyn MultiPass>>>) {
        if self
            .account
            .iter()
            .filter(|cs| cs.id() == handle.read().id())
            .count()
            != 0
        {
            return;
        }
        self.account.push(Account {
            handle,
            active: false,
        })
    }

    pub fn get_filesystem(&self) -> anyhow::Result<Arc<RwLock<Box<dyn Constellation>>>> {
        let index = self
            .filesystem
            .iter()
            .position(|item| item.active)
            .ok_or(Error::ArrayPositionNotFound)?;

        let fs = self.filesystem.get(index).ok_or(Error::Other)?;

        Ok(fs.as_ref().clone())
    }

    pub fn get_cache(&self) -> anyhow::Result<Arc<RwLock<Box<dyn PocketDimension>>>> {
        let index = self
            .cache
            .iter()
            .position(|item| item.active)
            .ok_or(Error::ArrayPositionNotFound)?;

        let cs = self.cache.get(index).ok_or(warp::error::Error::Other)?;

        Ok(cs.as_ref().clone())
    }

    pub fn get_account(&self) -> anyhow::Result<Arc<RwLock<Box<dyn MultiPass>>>> {
        let index = self
            .account
            .iter()
            .position(|item| item.active)
            .ok_or(Error::ArrayPositionNotFound)?;

        let cs = self.account.get(index).ok_or(Error::Other)?;

        Ok(cs.as_ref().clone())
    }
}

use std::collections::HashMap;

use crate::error::Error;

use warp_data::DataObject;
use warp_module::Module;

/// `Hook` contains identifying information about a given hook.
#[derive(Clone, PartialEq)]
pub struct Hook {
    pub name: String,
    pub module: Module,
}

/// Allows for creation of a new hook
impl Hook {
    pub fn new<S: AsRef<str>>(name: S, module: Module) -> Self {
        let name = name.as_ref().to_string();
        Self { name, module }
    }
}

/// Formats the Hook into a unique, human readable, identifier
impl ToString for Hook {
    fn to_string(&self) -> String {
        format!("{}::{}", self.module, self.name)
    }
}

/// Wraps the data from a given hook in the standard DataObject
pub type HookData = Box<dyn Fn(Hook, DataObject)>;

/// Lists all of the hooks registered
#[derive(Default)]
pub struct Hooks {
    pub hooks: Vec<Hook>,
    pub subscribers: HashMap<String, Vec<HookData>>,
}

/// General methods for using hooks throughout the platform.
impl Hooks {
    /// Create a new `Hook` registered to the system.
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_data::DataObject;
    ///     use warp_hooks::error::Error;
    ///     use warp_hooks::hooks::{Hook, Hooks};
    ///     use warp_module::Module;
    ///
    ///     let mut system = Hooks::default();
    ///     system.create("NEW_FILE", Module::FileSystem)?;
    ///     let hook = Hook::new("NEW_FILE", Module::FileSystem);
    /// ```
    pub fn create<S: AsRef<str>>(&mut self, name: S, module: Module) -> Result<(), Error> {
        let name = name.as_ref().to_owned();
        let hook = Hook::new(name, module);

        if self.hooks.contains(&hook) {
            return Err(Error::DuplicateHook);
        }

        self.hooks.push(hook);

        Ok(())
    }

    /// Get a list of hooks currently active in the system
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_data::DataObject;
    ///     use warp_hooks::error::Error;
    ///     use warp_hooks::hooks::{Hook, Hooks};
    ///     use warp_module::Module;
    ///
    ///     let mut system = Hooks::default();
    ///     system.create("NEW_FILE", Module::FileSystem)?;
    ///     let hooks = Hook::hooks();
    /// ```
    pub fn hooks(&self) -> Vec<Hook> {
        self.hooks.clone()
    }

    /// Subscribe to events on a given hook
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_data::DataObject;
    ///     use warp_hooks::error::Error;
    ///     use warp_hooks::hooks::{Hook, Hooks};
    ///     use warp_module::Module;
    ///
    ///     let mut system = Hooks::default();
    ///     system.create("NEW_FILE", Module::FileSystem)?;
    ///     system.subscribe(&hook, |hook, data| {
    ///         ssert_eq!(hook.name.as_str(), "NEW_FILE");
    ///         assert_eq!(hook.module, Module::FileSystem);
    ///         assert_eq!(data.module, Module::FileSystem);
    ///         let file: File = data.payload().unwrap();
    ///         assert_eq!(file.metadata.name.as_str(), "test.txt");
    ///     })?;
    /// ```
    pub fn subscribe<C: Fn(Hook, DataObject) + 'static>(
        &mut self,
        hook: &Hook,
        f: C,
    ) -> Result<(), Error> {
        if let Some(val) = self.subscribers.get_mut(&hook.to_string()) {
            val.push(Box::new(f))
        } else {
            self.subscribers.insert(hook.to_string(), vec![Box::new(f)]);
        }
        Ok(())
    }

    /// Trigger a hook to all subscribers
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp_data::DataObject;
    ///     use warp_hooks::error::Error;
    ///     use warp_hooks::hooks::{Hook, Hooks};
    ///     use warp_module::Module;
    ///
    ///     let mut system = Hooks::default();
    ///     system.create("NEW_FILE", Module::FileSystem)?;
    ///     system.emit("FILESYSTEM::NEW_FILE", &hook, &data);
    /// ```
    pub fn trigger<S: AsRef<str>>(&self, name: S, hook: &Hook, data: &DataObject) {
        if let Some(subscribers) = self.subscribers.get(name.as_ref()) {
            for subscriber in subscribers {
                subscriber(hook.clone(), data.clone());
            }
        }
    }
}

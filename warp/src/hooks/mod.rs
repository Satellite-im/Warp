use std::collections::HashMap;

use crate::error::Error;

use crate::data::{DataObject, DataType};
use crate::module::Module;

use crate::sync::*;

type Result<T> = std::result::Result<T, Error>;

pub trait HookType: std::fmt::Display {
    fn module_type(&self) -> DataType {
        DataType::Unknown
    }

    fn base_type(&self) -> String;

    fn hook_type(&self) -> String {
        format!("{}", self).to_lowercase()
    }
}

/// `Hook` contains identifying information about a given hook.
#[derive(Debug, Clone, PartialEq)]
pub struct Hook {
    /// Name of the hook/event
    name: String,

    /// Type in which the hook is meant for
    data_type: DataType,
}

impl<A> From<A> for Hook
where
    A: AsRef<str>,
{
    fn from(hook: A) -> Self {
        let mut hook = hook.as_ref().split("::");
        let (module_name, name) = (
            hook.next().unwrap_or_default(),
            hook.next().unwrap_or_default().to_string(),
        );
        let data_type = DataType::from(Module::from(module_name));
        Hook { name, data_type }
    }
}

/// Allows for creation of a new hook
impl Hook {
    pub fn new(module: Module, name: &str) -> Self {
        let name = name.to_string();
        Self {
            name,
            data_type: DataType::from(module),
        }
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn data_type(&self) -> DataType {
        self.data_type
    }
}

/// Formats the Hook into a unique, human readable, identifier
impl ToString for Hook {
    fn to_string(&self) -> String {
        format!("{}::{}", self.data_type, self.name)
    }
}

/// Wraps the data from a given hook in the standard DataObject
pub type HookData = Box<dyn Fn(&Hook, &DataObject) + Sync + Send>;

/// Lists all of the hooks registered
#[derive(Default, Clone)]
pub struct Hooks {
    /// List of hooks.
    hooks: Arc<Mutex<Vec<Hook>>>,

    /// A map of hooks with an array of executable functions
    subscribers: Arc<Mutex<HashMap<String, Vec<HookData>>>>,
}

/// Create a new `Hook` registered to the system.
///
/// # Examples
///
/// ```
///
///     use warp::hooks::{Hook, Hooks};
/// let systems = Hooks::from(vec!["filesystem::new_file"]);
///     assert_eq!(systems.list(), vec![Hook::from("filesystem::new_file")])
/// ```
impl<A: AsRef<str>> From<Vec<A>> for Hooks {
    fn from(h: Vec<A>) -> Self {
        let hooks = Arc::new(Mutex::new(h.iter().map(Hook::from).collect()));
        let subscribers = Arc::new(Mutex::new(HashMap::new()));
        Hooks { hooks, subscribers }
    }
}

/// General methods for using hooks throughout the platform.
impl Hooks {
    pub fn new() -> Self {
        Hooks::default()
    }

    /// Create `Hooks` instance from an array of `Hook` in string format
    ///
    ///
    /// # Examples
    ///
    /// ```
    ///
    ///     use warp::hooks::{Hook, Hooks};
    /// let systems = Hooks::from(vec!["filesystem::new_file"]);
    ///     assert_eq!(systems.list(), vec![Hook::from("filesystem::new_file")])
    pub fn new_from_vec(list: Vec<&str>) -> Self {
        Hooks::from(list)
    }

    /// Create a new `Hook` registered to the system.
    ///
    /// # Examples
    ///
    /// ```
    ///
    ///     use warp::hooks::{Hook, Hooks};
    /// use warp::module::Module;
    /// let mut system = Hooks::default();
    ///     let hook = system.create(Module::FileSystem, "new_file").unwrap();
    ///     assert_eq!(hook, Hook::new(Module::FileSystem, "new_file"));
    /// ```
    pub fn create(&mut self, module: Module, name: &str) -> Result<Hook> {
        let hook = Hook::new(module, name);
        let mut hooks = self.hooks.lock();
        if hooks.contains(&hook) {
            return Err(Error::DuplicateHook);
        }

        hooks.push(hook.clone());

        Ok(hook)
    }

    /// Get a list of hooks currently active in the system
    ///
    /// # Examples
    ///
    /// ```
    ///
    ///     use warp::hooks::Hooks;
    /// use warp::module::Module;
    /// let mut system = Hooks::default();
    ///     let hook = system.create(Module::FileSystem, "new_file").unwrap();
    ///     let hooks = system.list();
    ///     assert_eq!(hooks.get(0).unwrap(), &hook);
    /// ```
    pub fn list(&self) -> Vec<Hook> {
        self.hooks.lock().clone()
    }

    /// Subscribe to events on a given hook
    ///
    /// # Examples
    ///
    /// ```
    ///
    ///     use warp::constellation::file::File;
    /// use warp::data::{DataObject, DataType};
    /// use warp::hooks::{Hook, Hooks};
    /// use warp::module::Module;
    /// let mut system = Hooks::default();
    ///     let hook = system.create(Module::FileSystem, "new_file").unwrap();
    ///     //Check to see if hook already exist
    ///     assert_eq!(system.create(Module::FileSystem, "new_file").is_err(), true);
    ///     //Check to see if hook havent been registered
    ///     assert_eq!(system.subscribe(Hook::from("unknown::new_file"), |_, _|{}).is_err(), true);
    ///     system.subscribe("filesystem::new_file", |hook, data| {
    ///         assert_eq!(hook.name().as_str(), "new_file");
    ///         assert_eq!(hook.data_type(), DataType::from(Module::FileSystem));
    ///         let file: File = data.payload().unwrap();
    ///         assert_eq!(file.name().as_str(), "test.txt");
    ///     }).unwrap();
    ///     let data = DataObject::new(DataType::from(Module::FileSystem), File::new("test.txt")).unwrap();
    ///     system.trigger("filesystem::new_file", &data);
    /// ```
    pub fn subscribe<C, H>(&mut self, hook: H, f: C) -> Result<()>
    where
        C: 'static + Fn(&Hook, &DataObject) + Sync + Send,
        H: Into<Hook>,
    {
        let hook = hook.into();

        if !self.hooks.lock().contains(&hook) {
            return Err(Error::HookUnregistered);
        }

        self.subscribers
            .lock()
            .entry(hook.to_string())
            .or_insert_with(Vec::new)
            .push(Box::new(f));

        Ok(())
    }

    /// Trigger a hook to all subscribers
    ///
    /// # Examples
    ///
    /// ```
    ///     use warp::constellation::file::File;
    /// use warp::data::{DataObject, DataType};
    /// use warp::hooks::{Hook, Hooks};
    /// use warp::module::Module;
    ///     let mut system = Hooks::default();
    ///     let hook = system.create(Module::FileSystem, "new_file").unwrap();
    ///     system.subscribe("filesystem::new_file", |hook, data| {
    ///         assert_eq!(hook.name().as_str(), "new_file");
    ///         assert_eq!(hook.data_type(), DataType::from(Module::FileSystem));
    ///         let file: File = data.payload().unwrap();
    ///         assert_eq!(file.name().as_str(), "test.txt");
    ///     }).unwrap();
    ///     let data = DataObject::new(DataType::from(Module::FileSystem), File::new("test.txt")).unwrap();
    ///     system.trigger("filesystem::new_file", &data);
    /// ```
    pub fn trigger(&self, name: &str, data: &DataObject) {
        let hook = Hook::from(name);
        if let Some(subscribers) = self.subscribers.lock().get(name) {
            for subscriber in subscribers {
                subscriber(&hook, data);
            }
        }
    }
}

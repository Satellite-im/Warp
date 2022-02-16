use std::collections::HashMap;

use crate::error::Error;

use warp_data::DataObject;
use warp_module::Module;

#[derive(Clone, PartialEq)]
pub struct Hook {
    pub name: String,
    pub module: Module,
}

impl Hook {
    pub fn new<S: AsRef<str>>(name: S, module: Module) -> Self {
        let name = name.as_ref().to_string();
        Self { name, module }
    }
}

impl ToString for Hook {
    fn to_string(&self) -> String {
        format!("{}::{}", self.module, self.name)
    }
}

pub type HookData = Box<dyn Fn(Hook, DataObject)>;

#[derive(Default)]
pub struct Hooks {
    pub hooks: Vec<Hook>,
    pub subscribers: HashMap<String, Vec<HookData>>,
}

impl Hooks {
    pub fn create<S: AsRef<str>>(&mut self, name: S, module: Module) -> Result<(), Error> {
        let name = name.as_ref().to_owned();
        let hook = Hook::new(name, module);

        if self.hooks.contains(&hook) {
            return Err(Error::DuplicateHook);
        }

        self.hooks.push(hook);

        Ok(())
    }

    pub fn hooks(&self) -> Vec<Hook> {
        self.hooks.clone()
    }

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

    pub fn emit<S: AsRef<str>>(&self, name: S, hook: &Hook, data: &DataObject) {
        if let Some(subscribers) = self.subscribers.get(name.as_ref()) {
            for subscriber in subscribers {
                subscriber(hook.clone(), data.clone());
            }
        }
    }
}

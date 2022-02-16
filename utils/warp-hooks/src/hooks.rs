use std::collections::HashMap;

use crate::error::Error;

use warp_data::DataObject;
use warp_module::Module;

#[derive(Clone, PartialEq)]
pub struct Hook {
    name: String,
    module: Module,
}

pub type HookData = Box<dyn Fn(Hook, DataObject)>;

#[derive(Default)]
pub struct Hooks {
    pub hooks: Vec<Hook>,
    pub subscribers: HashMap<String, Vec<HookData>>,
}

pub fn hook_identifier(hook: &Hook) -> String {
    format!("{}::{}", hook.module, hook.name)
}

impl Hooks {
    pub fn create<S: AsRef<str>>(&mut self, name: S, module: Module) -> Result<(), Error> {
        let name = name.as_ref().to_owned();
        let hook = Hook { name, module };

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
        hook: Hook,
        f: C,
    ) -> Result<(), Error> {
        if let Some(val) = self.subscribers.get_mut(&hook_identifier(&hook)) {
            val.push(Box::new(f))
        } else {
            self.subscribers
                .insert(hook_identifier(&hook), vec![Box::new(f)]);
        }
        Ok(())
    }
}

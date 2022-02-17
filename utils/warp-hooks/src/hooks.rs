use std::collections::HashMap;

use crate::error::Error;

use warp_module::Module;
use warp_data::DataObject;

#[derive(Clone, PartialEq)]
pub struct Hook {
  name: String,
  module: Module,
}

pub struct HookData {
  hook: Hook,
  data: DataObject,
}

pub struct Hooks {
  pub hooks: Vec<Hook>,
  pub subscribers: HashMap<String, Vec<Box<dyn Fn(HookData)>>>,
}

pub fn hook_identifier(hook: &Hook) -> String {
  format!("{}::{}", hook.module.to_string(), hook.name)
}

impl Hooks {
  pub fn hooks(&self) -> Vec<Hook> {
    self.hooks.clone()
  }

  pub fn create(&mut self, name: &str, module: Module) -> Result<(), Error> {
    let hook = Hook {
      name: name.to_string(),
      module,
    };
  
    if self.hooks.contains(&hook) {
      return Err(Error::DuplicateHook);
    }
  
    self.hooks.push(hook);

    Ok(())
  }

  pub fn subscribe<C: 'static +  Fn(HookData)>(&mut self, hook: Hook, f: C) -> Result<(), Error> {
    if let Some(val) = self.subscribers.get_mut(&hook_identifier(&hook)) {
      val.push(Box::new(f))
    } else {
      self.subscribers.insert(hook_identifier(&hook), vec![Box::new(f)]);
    }
    Ok(())
  }
}
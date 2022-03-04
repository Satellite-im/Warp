#[cfg(test)]
mod test {
    use lazy_static::lazy_static;
    use std::sync::{Arc, Mutex};
    use warp_common::Result;
    use warp_constellation::file::File;
    use warp_data::DataObject;
    use warp_hooks::hooks::Hooks;
    use warp_module::Module;

    lazy_static! {
        pub static ref HOOKS: Arc<Mutex<Hooks>> = Arc::new(Mutex::new(Hooks::default()));
    }

    #[test]
    fn test() -> Result<()> {
        let mut system = HOOKS.lock().unwrap();

        let hook = system.create("NEW_FILE", Module::FileSystem)?;

        system.subscribe(hook, |hook, data| {
            assert_eq!(hook.name.as_str(), "NEW_FILE");
            assert_eq!(hook.module, Module::FileSystem);

            assert_eq!(data.module, Module::FileSystem);

            let file: File = data.payload().unwrap(); //TODO: Implement Result for `Fn`
            assert_eq!(file.name.as_str(), "test.txt");
        })?;
        let data = DataObject::new(&Module::FileSystem, File::new("test.txt"))?;

        system.trigger("FILESYSTEM::NEW_FILE", &data);
        Ok(())
    }
}

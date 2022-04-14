#[cfg(test)]
mod test {
    use lazy_static::lazy_static;
    use std::sync::{Arc, Mutex};
    use warp_common::Result;
    use warp_constellation::file::File;
    use warp_data::{DataObject, DataType};
    use warp_hooks::hooks::Hooks;
    use warp_module::Module;

    lazy_static! {
        pub static ref HOOKS: Arc<Mutex<Hooks>> = Arc::new(Mutex::new(Hooks::default()));
    }

    #[test]
    fn test() -> Result<()> {
        let mut system = HOOKS.lock().unwrap();

        let hook = system.create("new_file", Module::FileSystem)?;

        system.subscribe(hook, |hook, data| {
            assert_eq!(hook.name.as_str(), "new_file");
            assert_eq!(hook.data_type, DataType::Module(Module::FileSystem));

            assert_eq!(data.data_type, DataType::Module(Module::FileSystem));

            let file: File = data.payload().unwrap(); //TODO: Implement Result for `Fn`
            assert_eq!(file.name.as_str(), "test.txt");
        })?;
        let data = DataObject::new(DataType::Module(Module::FileSystem), File::new("test.txt"))?;

        system.trigger("filesystem::new_file", &data);
        Ok(())
    }
}

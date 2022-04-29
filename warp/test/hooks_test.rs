#[cfg(test)]
mod test {
    use lazy_static::lazy_static;
    use std::sync::{Arc, Mutex};
    use warp::constellation::file::File;
    use warp::data::{DataObject, DataType};
    use warp::hooks::Hooks;
    use warp::module::Module;
    use warp_common::Result;

    lazy_static! {
        pub static ref HOOKS: Arc<Mutex<Hooks>> = Arc::new(Mutex::new(Hooks::default()));
    }

    #[test]
    fn test() -> Result<()> {
        let mut system = HOOKS.lock().unwrap();

        let hook = system.create("new_file", Module::FileSystem)?;

        system.subscribe(hook, |hook, data| {
            assert_eq!(hook.name.as_str(), "new_file");
            assert_eq!(hook.data_type, DataType::from(Module::FileSystem));

            assert_eq!(data.data_type(), DataType::from(Module::FileSystem));

            let file: File = data.payload().unwrap(); //TODO: Implement Result for `Fn`
            assert_eq!(file.name(), String::from("test.txt"));
        })?;
        let data = DataObject::new(DataType::from(Module::FileSystem), File::new("test.txt"))?;

        system.trigger("filesystem::new_file", &data);
        Ok(())
    }
}

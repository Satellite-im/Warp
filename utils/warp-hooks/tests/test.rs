#[cfg(test)]
mod test {
    use warp_constellation::file::File;
    use warp_data::DataObject;
    use warp_hooks::error::Error;
    use warp_hooks::hooks::{Hook, Hooks};
    use warp_module::Module;

    #[test]
    fn test() -> Result<(), warp_hooks::error::Error> {
        let mut system = Hooks::default();

        system.create("NEW_FILE", Module::FileSystem)?;

        let hook = Hook::new("NEW_FILE", Module::FileSystem);

        let file = File::new("test.txt");
        system.subscribe(&hook, |hook, data| {
            assert_eq!(hook.name.as_str(), "NEW_FILE");
            assert_eq!(hook.module, Module::FileSystem);

            assert_eq!(data.module, Module::FileSystem);

            let file: File = data.payload().unwrap(); //TODO: Implement Result for `Fn`
            assert_eq!(file.metadata.name.as_str(), "test.txt");
        })?;
        let data = DataObject::new(&Module::FileSystem, file).map_err(|_| Error::Other)?;

        system.trigger("FILESYSTEM::NEW_FILE", &hook, &data);
        Ok(())
    }
}

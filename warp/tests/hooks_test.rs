#[cfg(test)]
mod test {
    use warp::constellation::file::File;
    use warp::data::{DataObject, DataType};
    use warp::hooks::Hooks;
    use warp::module::Module;

    #[test]
    fn test() -> anyhow::Result<()> {
        let mut system = Hooks::new();

        let hook = system.create(Module::FileSystem, "new_file")?;

        system.subscribe(hook, |hook, data| {
            assert_eq!(hook.name().as_str(), "new_file");
            assert_eq!(hook.data_type(), DataType::from(Module::FileSystem));

            assert_eq!(data.data_type(), DataType::from(Module::FileSystem));

            let file: File = data.payload().unwrap(); //TODO: Implement Result for `Fn`
            assert_eq!(file.name(), String::from("test.txt"));
        })?;
        let data = DataObject::new(DataType::from(Module::FileSystem), File::new("test.txt"))?;

        system.trigger("filesystem::new_file", &data);
        Ok(())
    }

    #[tokio::test]
    async fn test_shared() -> anyhow::Result<()> {
        let mut system = Hooks::new();

        let mut system_dup = system.clone();

        let hook = system.create(Module::FileSystem, "new_file")?;

        system.subscribe(hook, |hook, data| {
            assert_eq!(hook.name().as_str(), "new_file");
            assert_eq!(hook.data_type(), DataType::from(Module::FileSystem));

            assert_eq!(data.data_type(), DataType::from(Module::FileSystem));

            let file: File = data.payload().unwrap(); //TODO: Implement Result for `Fn`
            assert_eq!(file.name(), String::from("test.txt"));
        })?;
        let data = DataObject::new(DataType::from(Module::FileSystem), File::new("test.txt"))?;

        system.trigger("filesystem::new_file", &data);

        tokio::task::spawn_blocking(move || {
            system_dup.trigger("filesystem::new_file", &data);
        })
        .await?;
        Ok(())
    }
}

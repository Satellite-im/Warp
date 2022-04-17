#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use warp_common::chrono::{DateTime, Utc};
    use warp_common::serde::{Deserialize, Serialize};
    use warp_common::{Extension, Module};
    use warp_constellation::{directory::Directory, file::File};
    use warp_constellation::{Constellation, ConstellationDataType};

    #[derive(Serialize, Deserialize, Clone, Debug)]
    #[serde(crate = "warp_common::serde")]
    pub struct DummyFileSystem {
        index: Directory,
        modified: DateTime<Utc>,
        path: PathBuf,
    }

    impl Default for DummyFileSystem {
        fn default() -> Self {
            DummyFileSystem {
                index: Directory::new("root"),
                modified: Utc::now(),
                path: PathBuf::new(),
            }
        }
    }

    impl Extension for DummyFileSystem {
        fn id(&self) -> String {
            String::from("test")
        }
        
        fn name(&self) -> String {
            "Dummy Filesystem".to_string()
        }

        fn module(&self) -> Module {
            Module::FileSystem
        }
    }

    impl Constellation for DummyFileSystem {
        fn modified(&self) -> DateTime<Utc> {
            self.modified
        }

        fn root_directory(&self) -> &Directory {
            &self.index
        }

        fn root_directory_mut(&mut self) -> &mut Directory {
            &mut self.index
        }

        fn set_path(&mut self, path: PathBuf) {
            self.path = path;
        }

        fn get_path(&self) -> &PathBuf {
            &self.path
        }

        fn get_path_mut(&mut self) -> &mut PathBuf {
            &mut self.path
        }
    }

    #[test]
    fn test() -> warp_common::Result<()> {
        let mut filesystem = DummyFileSystem::default();

        let root = filesystem.open_directory("")?;

        root.add_child(File::new("testFile.png"))?;
        root.add_child(File::new("testPng2.png"))?;
        root.add_child(File::new("abc.png"))?;
        root.add_child(File::new("cc123.png"))?;
        root.add_child(Directory::new("Test Directory"))?;

        assert_eq!(root.has_child("testFile.png"), true);
        assert_eq!(root.has_child("testPng2.png"), true);
        assert_eq!(root.has_child("abc.png"), true);
        assert_eq!(root.has_child("cc123.png"), true);

        root.rename_child("abc.png", "test.png")?;

        assert_eq!(root.has_child("abc.png"), false);

        root.move_item_to("testFile.png", "Test Directory")?;

        assert_eq!(root.has_child("testFile.png"), false);
        assert_eq!(
            root.get_child_by_path("Test Directory/testFile.png")
                .is_ok(),
            true
        );

        Ok(())
    }

    #[test]
    fn can_import_export() -> warp_common::Result<()> {
        let mut filesystem = DummyFileSystem::default();

        let root = filesystem.open_directory("")?;

        root.add_child(File::new("testFile.png"))?;
        root.add_child(File::new("testPng2.png"))?;
        root.add_child(File::new("abc.png"))?;
        root.add_child(File::new("cc123.png"))?;

        assert_eq!(root.has_child("testFile.png"), true);
        assert_eq!(root.has_child("testPng2.png"), true);
        assert_eq!(root.has_child("abc.png"), true);
        assert_eq!(root.has_child("cc123.png"), true);

        // Json
        {
            let data = filesystem.export(ConstellationDataType::Json)?;
            let mut new_fs = DummyFileSystem::default();
            new_fs.import(ConstellationDataType::Json, data)?;
            assert_eq!(filesystem.root_directory().has_child("testPng2.png"), true);
        }

        // Yaml
        {
            let data = filesystem.export(ConstellationDataType::Yaml)?;
            let mut new_fs = DummyFileSystem::default();
            new_fs.import(ConstellationDataType::Yaml, data)?;
            assert_eq!(filesystem.root_directory().has_child("testFile.png"), true);
        }

        // Toml
        {
            let data = filesystem.export(ConstellationDataType::Toml)?;
            let mut new_fs = DummyFileSystem::default();
            new_fs.import(ConstellationDataType::Toml, data)?;
            assert_eq!(filesystem.root_directory().has_child("abc.png"), true);
        }

        Ok(())
    }
}

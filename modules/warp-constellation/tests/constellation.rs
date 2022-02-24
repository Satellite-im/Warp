#[cfg(test)]
mod tests {
    use warp_common::chrono::{DateTime, Utc};
    use warp_common::serde::{Deserialize, Serialize};
    use warp_constellation::constellation::{Constellation, ConstellationVersion};
    use warp_constellation::directory::{Directory, DirectoryType};

    #[derive(Serialize, Deserialize, Clone, Debug)]
    #[serde(crate = "warp_common::serde")]
    pub struct DummyFileSystem {
        version: ConstellationVersion,
        index: Directory,
        modified: DateTime<Utc>,
    }

    impl Default for DummyFileSystem {
        fn default() -> Self {
            DummyFileSystem {
                version: ConstellationVersion::from((0, 1, 2)),
                index: Directory::new("root"),
                modified: Utc::now(),
            }
        }
    }

    impl Constellation for DummyFileSystem {
        fn version(&self) -> &ConstellationVersion {
            &self.version
        }

        fn modified(&self) -> DateTime<Utc> {
            self.modified
        }

        fn root_directory(&self) -> &Directory {
            &self.index
        }

        fn root_directory_mut(&mut self) -> &mut Directory {
            &mut self.index
        }
    }

    #[test]
    fn test() -> warp_common::Result<()> {
        let mut filesystem = DummyFileSystem::default();

        filesystem.create_file("testFile.png")?;
        filesystem.create_file("testPng2.png")?;
        filesystem.create_file("abc.png")?;
        filesystem.create_file("cc123.png")?;
        filesystem.create_directory("Test Directory", DirectoryType::Default)?;

        assert_eq!(filesystem.has_child("testFile.png"), true);
        assert_eq!(filesystem.has_child("testPng2.png"), true);
        assert_eq!(filesystem.has_child("abc.png"), true);
        assert_eq!(filesystem.has_child("cc123.png"), true);

        let root = filesystem.open_directory("")?;

        root.rename_child("abc.png", "test.png")?;

        assert_eq!(root.has_child("abc.png"), false);

        root.move_item_to("testFile.png", "Test Directory")?;

        assert_eq!(root.has_child("testFile.png"), false);
        assert_eq!(
            root.get_child_by_path("Test Directory/testFile.png")
                .is_ok(),
            true
        );
        //TODO: Complete test
        Ok(())
    }
}

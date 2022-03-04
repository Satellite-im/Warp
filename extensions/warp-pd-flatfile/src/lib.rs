use std::path::Path;

#[derive(Default, Debug)]
pub struct FlatfileStorage {
    pub directory: String,
}

impl<P: AsRef<Path>> From<P> for FlatfileStorage {
    fn from(path: P) -> Self {
        //TODO: Possibly perform a check to assure that the path is actually a directory
        let path = path.as_ref();
        FlatfileStorage {
            directory: path.to_string_lossy().to_string(),
        }
    }
}

impl FlatfileStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self::default()
    }
}

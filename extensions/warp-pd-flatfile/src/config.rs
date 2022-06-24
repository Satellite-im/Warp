use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Config {
    pub directory: PathBuf,
    pub prefix: Option<String>,
    pub use_index_file: bool,
    pub index_file: Option<String>,
}

impl Config {
    pub fn development() -> Config {
        Config {
            directory: std::env::temp_dir(),
            index_file: Some("cache-index".into()),
            ..Default::default()
        }
    }
}

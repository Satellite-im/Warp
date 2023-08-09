mod config;

use super::Classifier;
use config::Config;
use serde::{Deserialize, Serialize};
use serde_json::{from_reader, to_writer};
use std::collections::HashSet;
use std::fs::File;
use std::io::Seek;
use std::path::Path;

const FILE_NAME: &str = "filter_keywords.json";

#[derive(Serialize, Deserialize)]
pub struct Keywords {
    blacklist: HashSet<String>,
    whitelist: HashSet<String>,
}
pub struct SimpleClassifier {
    keywords: Keywords,
    file_name: String,
}

#[allow(dead_code)]
impl SimpleClassifier {
    pub fn new() -> anyhow::Result<Self> {
        let file_name = FILE_NAME.to_string();

        if Path::new(&file_name).is_file() {
            let file = File::options().read(true).open(&file_name)?;
            let keywords: Keywords = from_reader(&file)?;

            Self::from_config(Config { keywords })
        } else {
            Self::from_config(Config::default())
        }
    }

    pub fn from_config(config: Config) -> anyhow::Result<Self> {
        let classifier = Self {
            keywords: config.keywords,
            file_name: FILE_NAME.to_string(),
        };
        Ok(classifier)
    }
}

impl Classifier for SimpleClassifier {
    fn process(&self, msg: &str) -> bool {
        let index = self
            .keywords
            .blacklist
            .iter()
            .position(|r| msg.to_lowercase().contains(&r.to_lowercase()));

        match index {
            None => false,
            Some(_) => !self.keywords.whitelist.contains(msg),
        }
    }

    fn score(&self, msg: &str) -> f32 {
        match self.process(msg) {
            true => 1.0,
            false => 0.0,
        }
    }

    fn add_spam(&mut self, msg: &str) {
        self.keywords.blacklist.insert(msg.to_string());
    }

    fn add_not_spam(&mut self, msg: &str) {
        self.keywords.whitelist.insert(msg.to_string());
    }

    fn save(&self) -> anyhow::Result<()> {
        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(&self.file_name)?;

        file.rewind()?;
        to_writer(&file, &self.keywords)?;

        Ok(())
    }
}

mod config;

use super::Classifier;
use bayespam;
use config::Config;
use std::fs::File;
use std::io::{Seek, Write};

const FILE_NAME: &str = "filter_model.json";

pub struct BayesClassifier {
    classifier: bayespam::classifier::Classifier,
    file_name: String,
}
#[allow(dead_code)]
impl BayesClassifier {
    pub fn new() -> anyhow::Result<Self> {
        let file_name = FILE_NAME.to_string();

        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(&file_name)?;

        if file.metadata()?.len() == 0 {
            file.write_all(include_bytes!("def_model.json"))?;
            file.rewind()?;
        }

        let classifier = bayespam::classifier::Classifier::new_from_pre_trained(&mut file)?;

        Ok(Self {
            classifier,
            file_name,
        })
    }

    pub fn from_config(config: Config) -> anyhow::Result<Self> {
        let mut file = File::options()
            .read(true)
            .write(true)
            .open(&config.file_name)?;

        let classifier = bayespam::classifier::Classifier::new_from_pre_trained(&mut file)?;

        Ok(Self {
            classifier,
            file_name: config.file_name,
        })
    }
}

impl Classifier for BayesClassifier {
    fn process(&self, msg: &str) -> bool {
        self.classifier.identify(msg)
    }

    fn score(&self, msg: &str) -> f32 {
        self.classifier.score(msg)
    }

    fn add_spam(&mut self, msg: &str) {
        self.classifier.train_spam(msg);
    }

    fn add_not_spam(&mut self, msg: &str) {
        self.classifier.train_ham(msg);
    }

    fn save(&self) -> anyhow::Result<()> {
        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(&self.file_name)?;

        self.classifier.save(&mut file, false)?;

        Ok(())
    }
}

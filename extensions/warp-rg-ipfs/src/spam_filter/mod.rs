pub mod classifiers;
mod default_blacklist;

#[allow(unused_imports)]
use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    fs::{File}
};
use warp::{error::Error};
use warp::{
    Extension,
};

use warp::module::Module;
use classifiers::{Classifier, BlacklistClassifier};

type Result<T> = std::result::Result<T, Error>;

pub struct SpamFilter {
    pub classifier: Box<dyn Classifier>,
}

impl SpamFilter {
    pub fn default() -> Self {
        SpamFilter::with_classifier(BlacklistClassifier::new())
    }

    pub fn with_classifier(classifier: Box<dyn Classifier>) -> Self {
        SpamFilter {
            classifier
        }
    }

    pub fn process(&self, str: &str) -> Result<bool> {
        Ok(self.classifier.process(str))
    }

    #[allow(dead_code)]
    pub fn add_spam(&mut self, str: &str) -> Result<()> {
        Ok(self.classifier.add_spam(str))
    }

    #[allow(dead_code)]
    pub fn score(&self, str: &str) -> Result<f32> {
        Ok(self.classifier.score(str))
    }
}

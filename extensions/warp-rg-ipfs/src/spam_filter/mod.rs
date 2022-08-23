pub mod classifiers;

use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    fs::{File}
};
use warp::{error::Error};
use warp::{Extension};
use warp::module::Module;

#[allow(unused_imports)]
use classifiers::{Classifier, SimpleClassifier, BayesClassifier};

type Result<T> = std::result::Result<T, Error>;

pub struct SpamFilter {
    pub classifier: Box<dyn Classifier>,
}

#[allow(dead_code)]
impl SpamFilter {
    pub fn default() -> anyhow::Result<Self> {
        Ok(SpamFilter::with_classifier(SimpleClassifier::new()?))
    }

    pub fn with_classifier(classifier: Box<dyn Classifier>) -> Self {
        SpamFilter {
            classifier
        }
    }

    pub fn process(&self, str: &str) -> Result<bool> {
        Ok(self.classifier.process(str))
    }

    pub fn add_spam(&mut self, str: &str) -> Result<()> {
        Ok(self.classifier.add_spam(str))
    }

    pub fn add_not_spam(&mut self, str: &str) -> Result<()> {
        Ok(self.classifier.add_not_spam(str))
    }

    pub fn score(&self, str: &str) -> Result<f32> {
        Ok(self.classifier.score(str))
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let _ = &self.classifier.save()?;
        Ok(())
    }
}

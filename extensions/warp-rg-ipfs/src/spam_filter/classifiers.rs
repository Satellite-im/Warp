use std::fs::File;
use std::io::{Seek};
use serde_json::{from_reader, to_writer};
use bayespam;
use crate::spam_filter::default_blacklist::DEFAULT_BLACKLIST;

pub trait Classifier: Send + Sync {
    fn process(&self, msg: &str) -> bool;
    fn score(&self, msg: &str) -> f32;
    fn add_spam(&mut self, msg: &str);
}

pub struct BayesClassifier {
    classifier: bayespam::classifier::Classifier,
    file_name: String,
}

impl BayesClassifier {
    #[allow(dead_code)]
    fn new() -> Self {
        let file_name = "spam_model.json".to_owned();
        let mut classifier = bayespam::classifier::Classifier::new();
        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(&file_name).unwrap();

        if file.metadata().unwrap().len() > 0 {
            classifier = bayespam::classifier::Classifier::new_from_pre_trained(&mut file).unwrap();
        }

        BayesClassifier {
            classifier,
            file_name,
        }
    }
}
impl Classifier for BayesClassifier {
    fn process(&self, str: &str) -> bool {
        self.classifier.identify(str)
    }

    fn score(&self, str: &str) -> f32 {
        self.classifier.score(str)
    }

    fn add_spam(&mut self, str: &str) {
        self.classifier.train_spam(str);
        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(&self.file_name).unwrap();
        self.classifier.save(&mut file, true).unwrap()
    }
}

pub struct BlacklistClassifier {
    keywords: Vec<String>,
    file_name: String,
}
impl BlacklistClassifier {
    pub fn new() -> Box<dyn Classifier> {
        let mut keywords: Vec<String> = serde_json::from_str(DEFAULT_BLACKLIST).unwrap();
        let file_name = "spam_blacklist.json".to_owned();
        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(&file_name).unwrap();

        if file.metadata().unwrap().len() > 0 {
            keywords = from_reader(&file).unwrap();
        } else {
            to_writer(&mut file, &keywords).unwrap();
        }

        let classifier = BlacklistClassifier {
            file_name,
            keywords,
        };
        Box::new(classifier)
    }
}
impl Classifier for BlacklistClassifier {
    fn process(&self, str: &str) -> bool {
        let index = self.keywords
            .iter()
            .position(|r| str.to_lowercase().contains(&r.to_lowercase()));

        match index {
            None => false,
            Some(_) => true,
        }
    }

    fn score(&self, str: &str) -> f32 {
        match self.process(str) {
            true => 1.0,
            false => 0.0,
        }
    }

    fn add_spam(&mut self, str: &str) {
        if !self.process(str) {
            self.keywords.push(str.to_owned());
            let mut file = File::options()
                .create(true)
                .read(true)
                .write(true)
                .open(&self.file_name).unwrap();
            file.rewind().unwrap();
            to_writer(&file, &self.keywords).unwrap()
        }
    }
}
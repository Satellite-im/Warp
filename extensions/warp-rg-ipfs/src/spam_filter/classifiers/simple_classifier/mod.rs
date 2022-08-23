use std::collections::HashSet;
use std::fs::File;
use std::io::{Seek};
use serde_json::{from_reader, to_writer};
use serde::{Deserialize, Serialize};
use super::Classifier;

#[derive(Serialize,Deserialize)]
pub struct Keywords {
    blacklist: HashSet<String>,
    whitelist: HashSet<String>,
}
pub struct SimpleClassifier {
    keywords: Keywords,
    file_name: String,
}
impl SimpleClassifier {
    pub fn new() -> anyhow::Result<Box<dyn Classifier>> {
        let mut keywords: Keywords = serde_json::from_str(include_str!("def_keywords.json"))?;
        let file_name = "filter_keywords.json".to_string();
        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(&file_name)?;

        if file.metadata().unwrap().len() > 0 {
            keywords = from_reader(&file)?;
        } else {
            to_writer(&mut file, &keywords)?;
        }

        let classifier = SimpleClassifier {
            file_name,
            keywords,
        };
        Ok(Box::new(classifier))
    }
}
impl Classifier for SimpleClassifier {
    fn process(&self, msg: &str) -> bool {
        let index = self.keywords.blacklist
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

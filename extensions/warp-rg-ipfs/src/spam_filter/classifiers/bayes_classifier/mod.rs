use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use bayespam;
use super::Classifier;

pub struct BayesClassifier {
    classifier: bayespam::classifier::Classifier,
    file_name: String,
}
#[allow(dead_code)]
impl BayesClassifier {
    pub fn new() -> anyhow::Result<Box<Self>> {
        let file_name = "spam_model.json".to_owned();

        let mut file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .open(&file_name)?;

        if file.metadata()?.len() == 0 {
            file.write(include_bytes!("def_model.json"))?;
            file.seek(SeekFrom::Start(0))?;
        }

        let classifier = bayespam::classifier::Classifier::new_from_pre_trained(&mut file)?;

        Ok(Box::new(BayesClassifier { classifier, file_name }))
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

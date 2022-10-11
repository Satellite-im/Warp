pub mod classifiers;

use warp::error::Error;

#[allow(unused_imports)]
use classifiers::{BayesClassifier, Classifier, SimpleClassifier};

type Result<T> = std::result::Result<T, Error>;

pub struct SpamFilter {
    pub classifier: Box<dyn Classifier>,
}

#[allow(dead_code)]
impl SpamFilter {
    pub fn default() -> anyhow::Result<Self> {
        Ok(SpamFilter::with_classifier(SimpleClassifier::new()?))
    }

    pub fn with_classifier(classifier: SimpleClassifier) -> Self {
        let classifier = Box::new(classifier);
        SpamFilter { classifier }
    }

    pub fn process(&self, str: &str) -> Result<bool> {
        Ok(self.classifier.process(str))
    }

    pub fn add_spam(&mut self, str: &str) -> Result<()> {
        self.classifier.add_spam(str);
        Ok(())
    }

    pub fn add_not_spam(&mut self, str: &str) -> Result<()> {
        self.classifier.add_not_spam(str);
        Ok(())
    }

    pub fn score(&self, str: &str) -> Result<f32> {
        Ok(self.classifier.score(str))
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let _ = &self.classifier.save()?;
        Ok(())
    }
}

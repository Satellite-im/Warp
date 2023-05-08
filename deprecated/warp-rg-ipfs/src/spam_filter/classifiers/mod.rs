mod bayes_classifier;
mod simple_classifier;

pub use bayes_classifier::*;
pub use simple_classifier::*;

pub trait Classifier: Send + Sync {
    fn process(&self, msg: &str) -> bool;
    fn score(&self, msg: &str) -> f32;
    fn add_spam(&mut self, msg: &str);
    fn add_not_spam(&mut self, msg: &str);
    fn save(&self) -> anyhow::Result<()>;
}

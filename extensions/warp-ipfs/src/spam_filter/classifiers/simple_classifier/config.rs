use super::Keywords;

pub struct Config {
    pub keywords: Keywords,
}

impl Default for Config {
    fn default() -> Self {
        let keywords: Keywords = serde_json::from_str(include_str!("def_keywords.json")).unwrap();
        Self { keywords }
    }
}

use std::path::PathBuf;

use serde::{Serialize, Deserialize};

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path {
    paths: Vec<String>,
}

impl From<Path> for std::path::PathBuf {
    fn from(value: Path) -> Self {
        PathBuf::from(value.to_string())
    }
}

impl core::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.paths.join("/"))
    }
}

impl<R: AsRef<str>> From<R> for Path {
    fn from(value: R) -> Self {
        let str = value.as_ref().to_owned();
        Path { paths: vec![str] }
    }
}

impl Path {
    #[inline]
    pub fn new<R: AsRef<str>>(p: R) -> Self {
        let item = p.as_ref().to_string();
        let paths = item.split('/').map(|s| s.to_string()).collect();
        Path { paths }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<String> {
        self.paths.pop()
    }

    #[inline]
    pub fn join<R: Into<Path>>(&self, item: R) -> Path {
        let item = item.into();
        let paths = item.iter().cloned().collect();
        Path { paths }
    }

    #[inline]
    pub fn push<R: AsRef<str>>(&mut self, item: R) {
        let item = item.as_ref().to_string();
        self.paths.push(item);
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.paths.iter()
    }
}

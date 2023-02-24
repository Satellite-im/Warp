use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
        write!(
            f,
            "{}{}",
            if self.paths.is_empty() { "" } else { "/" },
            self.paths.join("/")
        )
    }
}

impl<R: AsRef<str>> From<R> for Path {
    fn from(value: R) -> Self {
        let paths = value
            .as_ref()
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Path { paths }
    }
}

impl Path {
    #[inline]
    pub fn new<R: AsRef<str>>(p: R) -> Self {
        let paths = p
            .as_ref()
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        Path { paths }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<String> {
        self.paths.pop()
    }

    #[inline]
    pub fn join<R: Into<Path>>(&self, item: R) -> Path {
        let item = item.into();
        let paths = self.paths.iter().chain(item.iter()).cloned().collect();
        Path { paths }
    }

    #[inline]
    pub fn push<R: AsRef<str>>(&mut self, item: R) {
        let item = item.as_ref().to_string();
        self.paths.push(item);
    }

    #[inline]
    pub fn set_extension<S: AsRef<str>>(&mut self, extension: S) -> bool {
        let ext = extension.as_ref();
        if let Some(item) = self.paths.last_mut() {
            item.insert(item.len(), '.');
            item.insert_str(item.len(), ext);
            return true;
        }
        false
    }

    #[inline]
    pub fn first(&self) -> Option<&String> {
        self.paths.first()
    }

    #[inline]
    pub fn last(&self) -> Option<&String> {
        self.paths.last()
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.paths.iter()
    }
}

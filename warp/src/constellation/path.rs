use std::path::PathBuf;

use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path {
    paths: Vec<Bytes>,
}

impl Serialize for Path {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'d> Deserialize<'d> for Path {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'d>,
    {
        let str = <String>::deserialize(deserializer)?;
        let paths = str
            .split('/')
            .map(|s| Bytes::copy_from_slice(s.as_bytes()))
            .collect();
        Ok(Self { paths })
    }
}

impl From<Path> for std::path::PathBuf {
    fn from(value: Path) -> Self {
        PathBuf::from(value.to_string())
    }
}

impl core::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let iter = self
            .paths
            .iter()
            .map(|s| String::from_utf8_lossy(s).to_string());
        let path = itertools::join(iter, "/");
        write!(
            f,
            "{}{}",
            if self.paths.is_empty() { "" } else { "/" },
            path
        )
    }
}

impl<R: AsRef<str>> From<R> for Path {
    fn from(value: R) -> Self {
        let value = value.as_ref();
        let paths = value
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| Bytes::copy_from_slice(s.as_bytes()))
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
            .map(|s| Bytes::copy_from_slice(s.as_bytes()))
            .collect();
        Path { paths }
    }

    #[inline]
    pub fn pop(&mut self) -> Option<String> {
        self.paths
            .pop()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
    }

    #[inline]
    pub fn join<R: Into<Path>>(&self, item: R) -> Path {
        let item = item.into();
        let items = item
            .iter()
            .map(|s| Bytes::copy_from_slice(s.as_bytes()))
            .collect::<Vec<_>>();
        let paths = self.paths.iter().chain(items.iter()).cloned().collect();
        Path { paths }
    }

    #[inline]
    pub fn push<R: AsRef<str>>(&mut self, item: R) {
        let item = item.as_ref();
        let item = Bytes::copy_from_slice(item.as_bytes());
        self.paths.push(item);
    }

    #[inline]
    pub fn set_extension<S: AsRef<str>>(&mut self, extension: S) -> bool {
        let ext = extension.as_ref();
        if let Some(item) = self.paths.last_mut() {
            let mut item_str = String::from_utf8_lossy(item).to_string();
            item_str.insert(item_str.len(), '.');
            item_str.insert_str(item_str.len(), ext);
            let bytes = Bytes::copy_from_slice(item_str.as_bytes());
            *item = bytes;
            return true;
        }
        false
    }

    #[inline]
    pub fn first(&self) -> Option<String> {
        self.paths
            .first()
            .map(|bytes| String::from_utf8_lossy(bytes).to_string())
    }

    #[inline]
    pub fn last(&self) -> Option<String> {
        self.paths
            .last()
            .map(|bytes| String::from_utf8_lossy(bytes).to_string())
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = String> + '_ {
        self.paths
            .iter()
            .map(|bytes| String::from_utf8_lossy(bytes).to_string())
    }
}

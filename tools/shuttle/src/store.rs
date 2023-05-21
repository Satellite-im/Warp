use std::borrow::Cow;

pub mod ipfs;

#[async_trait::async_trait]
pub trait Store: Send + Sync + 'static {
    async fn insert(&mut self, key: &[u8], value: &[u8]) -> Result<(), anyhow::Error>;

    async fn find<'a>(&self, key: &[u8]) -> Result<Cow<'a, [u8]>, anyhow::Error>;

    async fn find_all<'a>(&self, key: &[u8]) -> Result<Vec<Cow<'a, [u8]>>, anyhow::Error>;

    async fn replace(&mut self, key: &[u8], value: &[u8]) -> Result<(), anyhow::Error>;

    async fn remove(&mut self, key: &[u8]) -> Result<(), anyhow::Error>;
}

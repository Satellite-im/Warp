use std::borrow::Cow;

pub mod ipfs;

#[async_trait::async_trait]
pub trait Store {
    type Type;
    type Channel;
    type Error;

    fn store_type(&self) -> Self::Type;

    async fn insert(&mut self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;

    async fn find<'a>(&self, key: &[u8]) -> Result<Cow<'a, [u8]>, Self::Error>;

    async fn replace(&mut self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;

    async fn remove(&mut self, key: &[u8]) -> Result<(), Self::Error>;

    async fn watch(&self) -> Self::Channel;
}

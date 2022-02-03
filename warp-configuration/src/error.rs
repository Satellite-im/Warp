use thiserror::Error;


#[derive(Error, Debug)]
pub enum Error {
    #[error("Cannot deserialize configuration: {0}")]
    TomlDeserializeError(#[from] toml::de::Error),
    #[error("Cannot serialize configuration: {0}")]
    TomlSerializeError(#[from] toml::ser::Error),
    #[error("{0}")]
    IoError(#[from] std::io::Error),
}
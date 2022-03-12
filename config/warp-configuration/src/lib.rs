pub mod error;

use crate::error::Error;
use serde::{Deserialize, Serialize}; // https://docs.serde.rs/serde/
use std::io::{Read, Write};
use std::path::Path;

// Acceptable module implementations for the FileSystem
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")] // https://serde.rs/container-attrs.html
pub enum FileSystem {
    Disk,
    Textile,
    WebTorrent,
}

// Acceptable module implementations for the Cache
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PocketDimension {
    FlatFile,
}

/// Represents options related to the REST API
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HTTPAPIConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// Defines which implementations to load for each module
#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleConfig {
    pub constellation: bool,
    pub pocket_dimension: bool,
    pub multipass: bool,
    pub raygun: bool
}

impl Default for ModuleConfig {
    fn default() -> Self {
        Self {
            constellation: true,
            pocket_dimension: true,
            multipass: false,
            raygun: false,
        }
    }
}

/// Define extensions to use at runtime
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct ExtensionConfig {
    pub constellation: Vec<String>,
    pub pocket_dimension: Vec<String>,
    pub multipass: Vec<String>,
    pub raygun: Vec<String>
}

/// Represents the global config for Warp
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub debug: bool,
    pub http_api: HTTPAPIConfig,
    pub modules: ModuleConfig,
    pub extensions: ExtensionConfig,
}

// Implementation to create, load and save the config
impl Config {
    /// Creates the configuration
    /// # Examples
    ///
    /// ```
    /// use warp_configuration::Config;
    /// let config = Config::new();
    /// ```
    pub fn new() -> Config {
        Config {
            debug: true,
            ..Default::default()
        }
    }

    /// Loads and return the parsed TOML configuration file for Warp
    /// # Examples
    ///
    /// ```no_run
    /// use warp_configuration::Config;
    /// let config = Config::load("Warp.toml").unwrap();
    /// ```
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Config, Error> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(Error::ConfigNotFound);
        }
        let mut file = std::fs::OpenOptions::new().read(true).open(path)?;

        Self::from_reader(&mut file)
    }

    /// Loads and return the parsed TOML configuration from reader
    /// # Examples
    ///
    /// ```no_run
    /// use warp_configuration::Config;
    /// use std::fs::File;
    ///
    /// let mut file = File::open("Warp.toml").unwrap();
    ///
    /// let config = Config::from_reader(&mut file).unwrap();
    /// ```
    pub fn from_reader<R: Read>(reader: &mut R) -> Result<Config, Error> {
        let mut data = String::new();
        reader.read_to_string(&mut data)?;
        toml::from_str(&data).map_err(Error::from)
    }

    /// Saves the configuration to disk
    /// # Examples
    ///
    /// ```no_run
    /// use warp_configuration::Config;
    /// let config = Config::new();
    /// config.save("Warp.toml").unwrap();
    /// ```
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)?;
        self.save_to_writer(&mut file)
    }

    /// Saves the configuration to writer
    /// # Examples
    ///
    /// ```no_run
    /// use warp_configuration::Config;
    /// use std::fs::File;
    ///
    /// let mut file = File::create("Warp.toml").unwrap();
    /// let config = Config::new();
    /// config.save_to_writer(&mut file).unwrap();
    /// ```
    pub fn save_to_writer<W: Write>(&self, writer: &mut W) -> Result<(), Error> {
        let config_data = toml::to_string(&self)?;
        writer.write_all(config_data.as_bytes())?;
        writer.flush()?;
        Ok(())
    }
}

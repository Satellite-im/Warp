pub mod error;

use std::io::{Read, Write};
use std::path::Path;
use serde::{Serialize, Deserialize}; // https://docs.serde.rs/serde/
use crate::error::Error;

// Acceptable module implementations for the FileSystem
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all="lowercase")] // https://serde.rs/container-attrs.html
pub enum FileSystem {
    Disk,
    Textile,
    WebTorrent
}

// Acceptable module implementations for the Cache
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all="lowercase")]
pub enum PocketDimension {
    FlatFile,
}

/// Represents options related to the REST API
#[derive(Debug, Serialize, Deserialize)]
pub struct HTTPAPIConfig {
    pub enabled: bool,
}

/// Defines which implementations to load for each module
#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleConfig {
    pub pocket_dimension: PocketDimension,
    pub file_system: FileSystem,
}
/// Represents the global config for Warp
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub debug: bool,
    pub http_api: HTTPAPIConfig,
    pub modules: ModuleConfig
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
            http_api: HTTPAPIConfig {
                enabled: false
            },
            modules: ModuleConfig {
                pocket_dimension: PocketDimension::FlatFile,
                file_system: FileSystem::Disk
            }
        }
    }

    /// Loads and return the parsed TOML configuration file for Warp
    /// # Examples
    ///
    /// ```
    /// use warp_configuration::Config;
    /// let config = Config::load("Warp.toml").unwrap();
    /// ```
    pub fn load<P: AsRef<Path>>(
        path: P
    ) -> Result<Config, Error> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(Error::ConfigNotFound);
        }
        let config_data = std::fs::read_to_string(path)?;
        toml::from_str(&config_data).map_err(Error::from)
    }

    /// Loads and return the parsed TOML configuration from reader
    /// # Examples
    ///
    /// ```
    /// use warp_configuration::Config;
    /// use std::fs::File;
    ///
    /// let mut file = File::open("Warp.toml").unwrap();
    ///
    /// let config = Config::from_reader(&mut file).unwrap();
    /// ```
    pub fn from_reader<R: Read>(
        reader: &mut R
    ) -> Result<Config, Error> {
        let mut data = String::new();
        reader.read_to_string(&mut data)?;
        toml::from_str(&data).map_err(Error::from)
    }

    /// Saves the configuration to disk
    /// # Examples
    ///
    /// ```
    /// use warp_configuration::Config;
    /// let config = Config::new();
    /// config.save("Warp.toml").unwrap();
    /// ```
    pub fn save<P: AsRef<Path>>(
        &self,
        path: P
    ) -> Result<(), Error> {
        let config_data = toml::to_string(&self)?;
        std::fs::write(path, config_data)?;
        Ok(())
    }

    /// Saves the configuration to writer
    /// # Examples
    ///
    /// ```
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
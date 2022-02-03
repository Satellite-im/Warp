use serde::{Deserialize}; // https://docs.serde.rs/serde/


// Acceptable module implementations for the FileSystem
#[derive(Deserialize)]
pub enum FileSystem {
    DISK,
}

// Acceptable module implementations for the Cache
#[derive(Deserialize)]
pub enum PocketDimension {
    FLATFILE,
}

/// Represents options related to the REST API
#[derive(Deserialize)]
pub struct APIConfig {
    pub enabled: bool,
}

/// Defines which implementations to load for each module
#[derive(Deserialize)]
pub struct ModuleConfig {
    pub pocket_dimension: PocketDimension,
    pub file_system: FileSystem,
}
/// Represents the global config for Warp
#[derive(Deserialize)]
pub struct Config {
    pub debug: bool,
    pub api: APIConfig,
    pub modules: ModuleConfig
}

/// Returns the parsed TOML config file for Warp
/// # Examples
///
/// ```
/// let cfg = warp_configuration::get().unwrap();
/// ```
pub fn get() -> Result<Config, Box<dyn std::error::Error>> {
    let local_config: String = std::fs::read_to_string("./Warp.toml")?;
    let config: Config = toml::from_str(&local_config).unwrap();
    return Ok(config);
}
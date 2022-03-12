use warp_configuration::Config;

fn enable_debug(config: &mut Config) -> Result<(), warp_configuration::error::Error> {
    config.debug = true;
    config.save("Warp.test.toml")
}

fn disable_debug(config: &mut Config) -> Result<(), warp_configuration::error::Error> {
    config.debug = false;
    config.save("Warp.test.toml")
}

fn main() -> Result<(), warp_configuration::error::Error> {
    let mut config = Config::new();
    config.save("Warp.test.toml")?;
    assert_eq!(config.debug, true);
    disable_debug(&mut config)?;
    assert_eq!(config.debug, false);
    enable_debug(&mut config)?;
    assert_eq!(config.debug, true);
    // std::fs::remove_file("Warp.test.toml")?;
    Ok(())
}

## Configuring Warp

**Lives At**: [Warp/warp-configuration](https://github.com/Satellite-im/Warp/tree/main/warp-configuration)

Warp allows you to pick and choose how it operates. Because of how it's built you have many options on how it operates behind the scenes, from choosing which protocol to run on, to if you'd like to expose a local API server.


#### Example Configuration

A configuration file will be generated for you automatically when you first run the project. If API keys are required for the modules you've chosen, the application will stop and ask you to fill them in. Warp does not require any API keys by default, however some module implementations might.

```toml
# Runtime Config Options

debug = true               # Enable verbose logging

[http_api]
enabled = false             # Enable HTTP API

# Compilation Config Options

[modules]
file_system = "disk"        # Which filesystem implementation to use
pocket_dimension = "flatfile"        # Which cacher implementation to use
```
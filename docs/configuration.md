## Configuring Wormhole

Wormhole allows you to pick and choose how it operates. Because of how it's built you have many options on how it operates behind the scenes, from choosing which protocol to run on, to if you'd like to expose a local API server.


#### Example Configuration

A configuration file will be generated for you automatically when you first run the project. If API keys are required for the modules you've chosen, the application will stop and ask you to fill them in. Wormhole does not require any API keys by default, however some module implementations might.

```toml
# Runtime Config Options

debug = true               # Enable verbose logging

[httpapi]
enabled = false             # Enable HTTP API

# Compilation Config Options

[mod]
filesystem = "disk"        # Which filesystem implementation to use
cacher = "flatfile"        # Which cacher implementation to use
```
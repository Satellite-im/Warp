## Configuring Wormhole

Wormhole allows you to pick and choose how it operates. Because of how it's built you have many options on how it operates behind the scenes, from choosing which protocol to run on, to if you'd like to expose a local API server.


#### Example Configuration

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
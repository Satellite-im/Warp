# Examples

Example usages of warp-ipfs

## Using from Rust (desktop)

CLI for interacting with Multipass (identity):
```
cargo run --example identity-interface
```

Basic ipfs setup example:
```
cargo run --example ipfs-example
```

Basic friends example:
```
cargo run --example ipfs-friends
```

Basic identity example:
```
cargo run --example ipfs-identity
```

CLI for interacting with Constellation (file management):
```
cargo run --example ipfs-persisent
```

CLI messenger example:
```
cargo run --example messenger
```

## Using from Rust (WASM)

[wasm-ipfs-friends](./wasm-ipfs-friends/README.md)

[wasm-ipfs-identity](./wasm-ipfs-identity/README.md)

[wasm-ipfs-storage](./wasm-ipfs-storage/README.md)

## Using from Javascript

Serves web files that contain examples of javascript calling into wasm built from `warp-ipfs`:
```
cargo run --example from-js
```
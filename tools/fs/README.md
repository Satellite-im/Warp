# fs

This crate provides functions to interact with the filesystem in a cross platform way. On `wasm32` targets it uses `LocalStorage`, otherwise it uses `tokio::fs`

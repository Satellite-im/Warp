#!/bin/sh -l
# Build the wasm pack
PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
echo "Starting wasm action"
cargo install wasm-pack
cd $GITHUB_WORKSPACE/extensions/warp-ipfs
echo "Building wasm-pack"
wasm-pack build --target web
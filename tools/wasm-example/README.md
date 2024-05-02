# wasm-example

## Compile & Run

```
cargo install wasm-pack
cargo install basic-http-server

wasm-pack build tools/wasm-example --target web --out-dir www/wasm/wasm-example
wasm-pack build warp --target web --out-dir ../tools/wasm-example/www/wasm/warp
wasm-pack build extensions/warp-ipfs --target web --out-dir ../../tools/wasm-example/www/wasm/warp-ipfs

basic-http-server tools/wasm-example/www
```
Then navigate to the `basic-wasm` example
# wasm-example

## Compile & Run

```
cargo install wasm-pack
cargo install basic-http-server

wasm-pack build tools/wasm-example --target web --out-dir ../../www/wasm/wasm-example
basic-http-server www
```

Then navigate to the `basic-wasm` example
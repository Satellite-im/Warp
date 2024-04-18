# wasm-ipfs-identity

To build do the following:

1. Install wasm32-unknown-unknown target by doing `rustup target add wasm32-unknown-unknown`
2. Install wasm-pack by doing `cargo install wasm-pack`
3. Run `wasm-pack build --target web --out-dir static`
4. Use a web server to serve the content from `static` directory. E.g with python install you can do `python3 -m http.server -d ./static`
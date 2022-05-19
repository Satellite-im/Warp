extern crate cbindgen;

use cbindgen::Language;
use std::env;
use std::str::FromStr;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR does not exist");
    let lang = Language::from_str(&env::var("CBINDGEN_LANG").unwrap_or_else(|_| String::from("C")))
        .unwrap_or(Language::C);

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_language(lang)
        .with_include_guard("_WARP_PD_FLATFILE_H_")
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("warp-pd-flatfile.h");
}

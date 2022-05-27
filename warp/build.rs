// extern crate cbindgen;
//
// use cbindgen::Language;
// use std::env;
// use std::str::FromStr;

#[cfg(feature = "build-header")]
fn main() {
    //install nightly

    let install_nightly_results = std::process::Command::new("rustup")
        .args(["install", "nightly"])
        .stdout(std::process::Stdio::inherit())
        .output()
        .unwrap();

    println!("cargo:warning={:?}", install_nightly_results);

    let install_cbindgen_results = std::process::Command::new("cargo")
        .args(["install", "cbindgen", "--force"])
        .stdout(std::process::Stdio::inherit())
        .output()
        .unwrap();

    println!("cargo:warning={:?}", install_cbindgen_results);

    let run_cbindgen_results = std::process::Command::new("rustup")
        .args([
            "run",
            "nightly",
            "--",
            "cbindgen",
            "-c",
            "cbindgen.toml",
            "-o",
            "warp.h",
        ])
        .stdout(std::process::Stdio::inherit())
        .output()
        .unwrap();

    println!("cargo:warning={:?}", run_cbindgen_results.status);
    // let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR does not exist");
    // let lang = Language::from_str(&env::var("CBINDGEN_LANG").unwrap_or_else(|_| String::from("C")))
    //     .unwrap_or(Language::C);
    //
    // cbindgen::Builder::new()
    //     .with_crate(crate_dir)
    //     .with_language(lang)
    //     .with_include_guard("_WARP_H_")
    //     .generate()
    //     .expect("Unable to generate bindings")
    //     .write_to_file("warp.h");
}

#[cfg(not(feature = "build-header"))]
fn main() {}

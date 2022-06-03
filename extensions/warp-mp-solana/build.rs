const Config: &'static str = r#"

language = "C"

cpp_compat = true
include_guard = "_WARP_MP_SOLANA_H_"

[export]

exclude = ["FFIError", "FFIResult_c_char"]

[parse]
parse_deps = true
clean = false
include = ["warp"]

[parse.expand]
crates = ["warp", "warp-mp-solana"]

"#;

#[cfg(feature = "build-header")]
fn main() {
    std::fs::write("cbindgen.toml", Config).unwrap();
    println!("cargo:warning=Running `cbindgen`");
    let run_cbindgen_results = std::process::Command::new("rustup")
        .args([
            "run",
            "nightly",
            "--",
            "cbindgen",
            "-c",
            "cbindgen.toml",
            "-o",
            "warp-mp-solana.h",
        ])
        .stdout(std::process::Stdio::inherit())
        .output()
        .unwrap();

    println!(
        "cargo:warning=Status Success:{}",
        run_cbindgen_results.status.success()
    );
}

#[cfg(not(feature = "build-header"))]
fn main() {}

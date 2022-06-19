const CONFIG: &'static str = r#"
language = "C"

cpp_compat = true

[parse]
parse_deps = false
clean = false

[parse.expand]
crates = ["warp"]
"#;

#[cfg(feature = "build-header")]
fn main() {
    std::fs::write("cbindgen.toml", CONFIG).unwrap();
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
            "warp-pd-stretto.h",
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

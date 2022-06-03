const Config: &'static str = r#"
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
    std::fs::write("cbindgen.toml", Config).unwrap();
    println!("cargo:warning=Running `rustup run nightly -- cbindgen -c cbindgen.toml -o warp.h`");
    let run_cbindgen_results = std::process::Command::new("rustup")
        .args([
            "run",
            "nightly",
            "--",
            "cbindgen",
            "-c",
            "cbindgen.toml",
            "-o",
            "warp-pd-memory.h",
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

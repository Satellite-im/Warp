#[allow(dead_code)]
static CONFIG: &str = r#"
language = "C"

cpp_compat = true

[export]

exclude = ["FFIError", "FFIResult_c_char"]

[parse]
parse_deps = true
clean = false
include = ["warp"]

[parse.expand]
crates = ["warp", "warp-pd-flatfile"]

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
            "warp-pd-flatfile.h",
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

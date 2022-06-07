const CONFIG: &'static str = r#"

language = "C"

cpp_compat = true
include_guard = "_WARP_RG_LIBP2P_H_"

[export]

exclude = ["FFIError"]

[parse]
parse_deps = true
clean = false
include = ["warp"]

[parse.expand]
crates = ["warp", "warp-rg-libp2p"]


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
            "warp-rg-libp2p.h",
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

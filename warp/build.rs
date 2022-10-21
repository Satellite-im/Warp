// extern crate cbindgen;
//
// use cbindgen::Language;
// use std::env;
// use std::str::FromStr;

#[cfg(feature = "build-header")]
fn main() {
    if cfg!(feature = "force-install") {
        //install nightly

        println!("cargo:warning=Running `rustup install nightly`");
        let install_nightly_results = std::process::Command::new("rustup")
            .args(["install", "nightly"])
            .stdout(std::process::Stdio::inherit())
            .output()
            .unwrap();

        println!(
            "cargo:warning=Status Success:{}",
            install_nightly_results.status.success()
        );

        println!("cargo:warning=Running `cargo install cbindgen`");

        let install_cbindgen_results = std::process::Command::new("cargo")
            .args(["install", "cbindgen"])
            .arg("--force")
            .stdout(std::process::Stdio::inherit())
            .output()
            .unwrap();

        println!(
            "cargo:warning=Status Success:{}",
            install_cbindgen_results.status.success()
        );
    }

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
            "warp.h",
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
fn main() {
    // let lines = r#"
    // If you wish to generate a `warp.h` header file for warp crate, please install cbindgen by using the following

    //     rustup install nightly
    //     cargo install cbindgen
    //     cargo build --features "build-header"

    // Alternatively, you can run the following

    //     cargo build --features "build-header force-install"

    // which will attempt to run and install the commands mentioned aboved but with no guarantee of success.

    // This may change in the future to where this is not needed.
    // "#.split('\n').collect::<Vec<_>>();

    // for line in lines {
    //     println!("cargo:note={}", line);
    // }
}

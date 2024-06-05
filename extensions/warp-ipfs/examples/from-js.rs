use env_logger::{Builder, Env};
use std::process::Command;
use tiny_file_server::FileServer;

const ADDR: &str = "127.0.0.1:9080";
const PATH: &str = "extensions/warp-ipfs/examples/from-js";

fn main() {
    println!("\nInstalling wasm-pack ...");
    let cmd = get_cmd("cargo install wasm-pack");
    spawn_and_wait(cmd);

    println!("\nBuilding warp-ipfs wasm files ...");
    let cmd = get_cmd("wasm-pack build extensions/warp-ipfs --target web --out-dir examples/from-js/built-wasm/warp-ipfs");
    spawn_and_wait(cmd);

    println!("\nStarting file server ...");
    Builder::from_env(Env::default().default_filter_or("info")).init();
    FileServer::http(ADDR)
        .expect("Server should be created")
        .run(PATH)
        .expect("Server should start");
}

// assumes all spaces are argument separators. args containing spaces will yield unexpected results (such as strings)
fn get_cmd(cmd_str: &str) -> Command {
    let mut split = cmd_str.split(" ");

    // first item is the program, then the rest of the items are the args
    let mut cmd = Command::new(split.nth(0).unwrap());
    for arg in split {
        cmd.arg(arg);
    }
    cmd
}

fn spawn_and_wait(mut cmd: Command) {
    let status = cmd
        .spawn()
        .expect("command failed to start")
        .wait()
        .expect("failed to get ExitStatus");

    if !status.success() {
        panic!("cmd ExitStatus not successful");
    };
}

pub mod config;

fn main() {
  let cfg: config::Config = config::get().unwrap();
  
}

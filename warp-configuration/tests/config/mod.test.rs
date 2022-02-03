pub mod config;

#[cfg(test)]
mod tests {
  let cfg = config::get().unwrap();

  #[test]
  fn loads() {
    assert_ne!(cfg, unreachable!);
  }
}
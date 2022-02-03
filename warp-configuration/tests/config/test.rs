
#[cfg(test)]
mod tests {
  let cfg = warp_configuration::get().unwrap();

  #[test]
  fn loads() {
    assert_ne!(cfg, unreachable!);
  }
}
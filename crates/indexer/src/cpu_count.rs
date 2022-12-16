lazy_static::lazy_static! {
  pub static ref CPU_COUNT: Option<usize> = {
      std::env::args()
      .nth(4)
      .map(|num| num.parse::<usize>().unwrap())
  };
}

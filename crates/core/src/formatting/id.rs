use std::cell::UnsafeCell;
use std::thread::LocalKey;

#[derive(Default)]
pub struct IdCounter(UnsafeCell<usize>);

impl IdCounter {
  pub fn next(k: &'static LocalKey<Self>) -> usize {
    k.with(|IdCounter(inner)| unsafe {
      let n = inner.get();
      *n += 1;
      *n
    })
  }
}

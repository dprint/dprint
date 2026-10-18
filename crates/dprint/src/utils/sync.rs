use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

#[derive(Default)]
pub struct AtomicCounter(AtomicUsize);

impl AtomicCounter {
  pub fn inc(&self) {
    self.0.fetch_add(1, Ordering::SeqCst);
  }

  pub fn get(&self) -> usize {
    self.0.load(Ordering::SeqCst)
  }
}

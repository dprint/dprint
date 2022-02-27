use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct Poisoner(Arc<AtomicBool>);

impl Poisoner {
  pub fn poison(&self) {
    self.0.store(true, Ordering::SeqCst);
  }

  pub fn is_poisoned(&self) -> bool {
    self.0.load(Ordering::SeqCst)
  }
}

#[derive(Default, Clone)]
pub struct IdGenerator(Arc<AtomicU32>);

impl IdGenerator {
  pub fn next(&self) -> u32 {
    self.0.fetch_add(1, Ordering::SeqCst)
  }
}

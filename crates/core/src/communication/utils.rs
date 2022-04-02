use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use parking_lot::Mutex;

use tokio_util::sync::CancellationToken;
use tokio_util::sync::WaitForCancellationFuture;

#[derive(Default, Clone)]
pub struct Poisoner(Arc<CancellationToken>);

impl Poisoner {
  pub fn poison(&self) {
    self.0.cancel()
  }

  pub fn is_poisoned(&self) -> bool {
    self.0.is_cancelled()
  }

  pub fn wait_poisoned(&self) -> WaitForCancellationFuture {
    self.0.cancelled()
  }
}

#[derive(Default, Clone)]
pub struct ArcFlag(Arc<AtomicBool>);

impl ArcFlag {
  pub fn raise(&self) {
    self.0.store(true, Ordering::SeqCst)
  }

  pub fn is_raised(&self) -> bool {
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

/// A store that can be shared across multiple threads, keyed by id.
pub struct ArcIdStore<T>(Arc<Mutex<HashMap<u32, T>>>);

impl<T> Default for ArcIdStore<T> {
  fn default() -> Self {
    Self(Default::default())
  }
}

impl<T> ArcIdStore<T> {
  pub fn new() -> Self {
    Default::default()
  }

  pub fn store(&self, message_id: u32, data: T) {
    self.0.lock().insert(message_id, data);
  }

  pub fn take(&self, message_id: u32) -> Option<T> {
    self.0.lock().remove(&message_id)
  }
}

impl<T: Clone> ArcIdStore<T> {
  pub fn get_cloned(&self, message_id: u32) -> Option<T> {
    self.0.lock().get(&message_id).cloned()
  }
}

// the #[derive(Clone)] macro wasn't working with the type parameter properly
// https://github.com/rust-lang/rust/issues/26925
impl<T> Clone for ArcIdStore<T> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

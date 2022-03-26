use std::collections::HashMap;
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
pub struct IdGenerator(Arc<AtomicU32>);

impl IdGenerator {
  pub fn next(&self) -> u32 {
    self.0.fetch_add(1, Ordering::SeqCst)
  }
}

/// A store that can be shared across multiple threads, keyed by id.
pub struct ArcIdStore<T>(Arc<Mutex<HashMap<u32, T>>>);

impl<T> ArcIdStore<T> {
  pub fn new() -> Self {
    Self(Default::default())
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

// not sure why, but I needed to manually implement this because
// of the type parameter
impl<T> Clone for ArcIdStore<T> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

/// Functionality to exit the process when a panic occurs.
/// This automatically happens when handling process plugin messages.
pub fn setup_exit_process_panic_hook() {
  // tokio doesn't exit on task panic, so implement that behaviour here
  let orig_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |panic_info| {
    orig_hook(panic_info);
    std::process::exit(1);
  }));
}

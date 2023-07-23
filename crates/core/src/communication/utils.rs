use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[derive(Default)]
pub struct AtomicFlag(AtomicBool);

impl AtomicFlag {
  pub fn raise(&self) -> bool {
    !self.0.swap(true, Ordering::SeqCst)
  }

  pub fn is_raised(&self) -> bool {
    self.0.load(Ordering::SeqCst)
  }
}

#[derive(Default)]
pub struct IdGenerator(RefCell<u32>);

impl IdGenerator {
  pub fn next(&self) -> u32 {
    let mut borrow = self.0.borrow_mut();
    let next = *borrow;
    *borrow += 1;
    next
  }
}

pub struct RcIdStoreGuard<'a, T> {
  store: &'a RcIdStore<T>,
  message_id: u32,
}

impl<'a, T> Drop for RcIdStoreGuard<'a, T> {
  fn drop(&mut self) {
    self.store.take(self.message_id);
  }
}

/// A store keyed by id.
pub struct RcIdStore<T>(Rc<RefCell<HashMap<u32, T>>>);

impl<T> Default for RcIdStore<T> {
  fn default() -> Self {
    Self(Default::default())
  }
}

impl<T> RcIdStore<T> {
  pub fn new() -> Self {
    Default::default()
  }

  pub fn store(&self, message_id: u32, data: T) {
    self.0.borrow_mut().insert(message_id, data);
  }

  pub fn store_with_guard(&self, message_id: u32, data: T) -> RcIdStoreGuard<'_, T> {
    self.store(message_id, data);
    RcIdStoreGuard { store: self, message_id }
  }

  pub fn take(&self, message_id: u32) -> Option<T> {
    self.0.borrow_mut().remove(&message_id)
  }

  pub fn take_all(&self) -> HashMap<u32, T> {
    let mut map = self.0.borrow_mut();
    std::mem::take(&mut *map)
  }
}

impl<T: Clone> RcIdStore<T> {
  pub fn get_cloned(&self, message_id: u32) -> Option<T> {
    self.0.borrow().get(&message_id).cloned()
  }
}

// the #[derive(Clone)] macro wasn't working with the type parameter properly
// https://github.com/rust-lang/rust/issues/26925
impl<T> Clone for RcIdStore<T> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

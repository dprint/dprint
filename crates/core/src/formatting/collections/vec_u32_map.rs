/// A map that is backed by a vector. It has super fast
/// inserts and lookups, but uses more memory.
pub struct VecU32Map<T: Clone>(Vec<Option<T>>);

impl<T: Clone> VecU32Map<T> {
  pub fn new(capacity: u32) -> Self {
    let capacity = capacity as usize;
    let mut vec = Vec::with_capacity(capacity);
    vec.resize(capacity, None);
    Self(vec)
  }

  pub fn insert(&mut self, key: u32, value: T) {
    let key = key as usize;
    if self.0.len() < key + 1 {
      if cfg!(debug_assertions) {
        panic!("DEBUG PANIC: A VecU32Map should ideally never be resized. Make sure you give it the correct capacity.");
      }
      self.0.resize(key, None);
    }
    self.0[key] = Some(value);
  }

  pub fn remove(&mut self, key: u32) {
    if let Some(value) = self.0.get_mut(key as usize) {
      *value = None;
    }
  }

  pub fn get(&self, key: u32) -> Option<&T> {
    self.0.get(key as usize).and_then(|v| v.as_ref())
  }
}

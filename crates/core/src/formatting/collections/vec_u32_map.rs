/// A map that is backed by a vector. It has super fast
/// inserts and lookups, but uses more memory.
pub struct VecU32Map<T: Clone>(VecU32MapWithDefault<Option<T>>);

impl<T: Clone> VecU32Map<T> {
  pub fn with_capacity(capacity: u32) -> Self {
    Self(VecU32MapWithDefault::with_capacity(capacity))
  }

  pub fn insert(&mut self, key: u32, value: T) {
    self.0.set(key, Some(value))
  }

  pub fn remove(&mut self, key: u32) {
    self.0.set(key, None);
  }

  pub fn get(&self, key: u32) -> Option<&T> {
    self.0.get(key).and_then(|v| v.as_ref())
  }
}

/// Faster than a hash map for when some data is stored
/// by an incrementing count from 0..n and most of that
/// data will be stored.
///
/// Use VecU32Map if you are storing some data optionally.
pub struct VecU32MapWithDefault<T: Clone + Default>(Vec<T>);

impl<T: Clone + Default> VecU32MapWithDefault<T> {
  pub fn with_capacity(capacity: u32) -> Self {
    let capacity = capacity as usize;
    let mut vec = Vec::with_capacity(capacity);
    vec.resize(capacity, Default::default());
    Self(vec)
  }

  pub fn set(&mut self, key: u32, value: T) {
    let key = key as usize;
    if self.0.len() < key + 1 {
      if cfg!(debug_assertions) {
        panic!("DEBUG PANIC: A VecU32Map should ideally never be resized. Make sure you give it the correct capacity.");
      }
      self.0.resize(key, Default::default());
    }
    self.0[key] = value;
  }

  pub fn get(&self, key: u32) -> Option<&T> {
    self.0.get(key as usize)
  }
}

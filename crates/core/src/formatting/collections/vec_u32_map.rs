use std::fmt::Display;

// These maps are backed by a vector and so they're super fast
// for lookups. In our case, we have a bunch of incrementing
// identifiers going from 0..n and so we can store these values
// efficiently with fast lookups in a vector (hashing was slow)

/// More efficient memory storage version of for
/// u32 values where it uses u32::MAX to mean None.
pub struct VecU32U32Map(VecU32MapWithValuedNone<u32>);

impl VecU32U32Map {
  pub fn with_capacity(capacity: u32) -> Self {
    Self(VecU32MapWithValuedNone::with_capacity_and_default(capacity, u32::MAX))
  }

  pub fn insert(&mut self, key: u32, value: u32) {
    self.0.insert(key, value)
  }

  pub fn remove(&mut self, key: u32) {
    self.0.remove(key);
  }

  pub fn get(&self, key: u32) -> Option<u32> {
    self.0.get(key)
  }
}

/// More efficient memory storage version of VecU32Map for
/// u8 values where it uses u8::MAX to mean None.
pub struct VecU32U8Map(VecU32MapWithValuedNone<u8>);

impl VecU32U8Map {
  pub fn with_capacity(capacity: u32) -> Self {
    Self(VecU32MapWithValuedNone::with_capacity_and_default(capacity, u8::MAX))
  }

  pub fn insert(&mut self, key: u32, value: u8) {
    self.0.insert(key, value)
  }

  pub fn remove(&mut self, key: u32) {
    self.0.remove(key);
  }

  pub fn get(&self, key: u32) -> Option<u8> {
    self.0.get(key)
  }
}

/// More efficient memory storage version of VecU32Map for
/// bool values where the first 4 bytes are if the value is set and
/// the second 4 are the values.
///
/// This was made mainly for fun.
pub struct VecU32BoolMap(Vec<u8>);

impl VecU32BoolMap {
  pub fn with_capacity(capacity: u32) -> Self {
    let len = (capacity / 4 + if capacity % 4 == 0 { 0 } else { 1 }) as usize;
    Self(vec![0; len])
  }

  pub fn insert(&mut self, key: u32, value: bool) {
    let byte_index = key as usize / 4;

    if byte_index >= self.0.len() {
      if cfg!(debug_assertions) {
        panic!("DEBUG PANIC: A VecU32BoolMap should ideally never be resized. Make sure you give it the correct capacity.");
      }
      self.0.resize(byte_index + 1, 0);
    }

    let option_bit_index = key % 4;
    let value_bit_index = option_bit_index + 4;
    let byte = self.0[byte_index];
    let byte = byte | (1 << option_bit_index); // set on
    let byte = if value {
      byte | (1 << value_bit_index)
    } else {
      byte & !(1 << value_bit_index)
    };
    self.0[byte_index] = byte;
  }

  pub fn remove(&mut self, key: u32) {
    let byte_index = key as usize / 4;
    if byte_index < self.0.len() {
      let option_bit_index = key % 4;
      let byte = self.0[byte_index];
      let byte = byte & !(1 << option_bit_index); // set off
      self.0[byte_index] = byte;
    }
  }

  pub fn get(&self, key: u32) -> Option<bool> {
    let byte_index = key as usize / 4;
    if byte_index < self.0.len() {
      let option_bit_index = key % 4;
      let byte = self.0[byte_index];
      if ((byte >> option_bit_index) & 1) == 1 {
        let value_bit_index = option_bit_index + 4;
        Some(((byte >> value_bit_index) & 1) == 1)
      } else {
        None
      }
    } else {
      None
    }
  }
}

/// A map that uses a value (ex. u32::MAX) to mean `None`.
/// This can be more memory efficient by not requiring extra data for the Option
/// especially in this scenario where the max value will likely never be hit.
struct VecU32MapWithValuedNone<T: Clone + Copy + PartialEq + Display>(VecU32MapWithDefault<T>);

impl<T: Clone + Copy + PartialEq + Display> VecU32MapWithValuedNone<T> {
  pub fn with_capacity_and_default(capacity: u32, default: T) -> Self {
    Self(VecU32MapWithDefault::with_capacity_and_default(capacity, default))
  }

  pub fn insert(&mut self, key: u32, value: T) {
    if value == self.0.default {
      panic!("Exceeded maximum value of {} for key {}", self.0.default, key);
    }
    self.0.set(key, value)
  }

  pub fn remove(&mut self, key: u32) {
    self.0.set(key, self.0.default);
  }

  pub fn get(&self, key: u32) -> Option<T> {
    let value = *self.0.get(key)?;
    if value == self.0.default {
      None
    } else {
      Some(value)
    }
  }
}

/// Faster than a hash map for when some data is stored
/// by an incrementing count from 0..n and most of that
/// data will be stored.
///
/// Use VecU32Map if you are storing some data optionally.
pub struct VecU32MapWithDefault<T: Clone> {
  vec: Vec<T>,
  default: T,
}

impl<T: Clone + Default> VecU32MapWithDefault<T> {
  pub fn with_capacity(capacity: u32) -> Self {
    Self::with_capacity_and_default(capacity, Default::default())
  }
}

impl<T: Clone> VecU32MapWithDefault<T> {
  pub fn with_capacity_and_default(capacity: u32, default: T) -> Self {
    let capacity = capacity as usize;
    let mut vec = Vec::with_capacity(capacity);
    vec.resize(capacity, default.clone());
    Self { vec, default }
  }

  pub fn set(&mut self, key: u32, value: T) {
    let key = key as usize;
    if key >= self.vec.len() {
      if cfg!(debug_assertions) {
        panic!("DEBUG PANIC: A VecU32Map should ideally never be resized. Make sure you give it the correct capacity.");
      }
      self.vec.resize(key + 1, self.default.clone());
    }
    self.vec[key] = value;
  }

  pub fn get(&self, key: u32) -> Option<&T> {
    self.vec.get(key as usize)
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn support_map_with_valued_none() {
    let mut map = VecU32U8Map::with_capacity(1);
    map.insert(0, u8::MAX - 1);
    assert_eq!(map.get(0), Some(u8::MAX - 1));
    map.remove(0);
    assert_eq!(map.get(0), None);
  }

  #[test]
  #[should_panic(expected = "Exceeded maximum value of 255 for key 0")]
  fn panic_with_map_with_valued_none_and_max_val() {
    let mut map = VecU32U8Map::with_capacity(1);
    map.insert(0, u8::MAX);
  }

  #[test]
  fn support_u32_bool_map() {
    for i in 0..12 {
      let mut map = VecU32BoolMap::with_capacity(i);
      for i in 0..i {
        assert_eq!(map.get(i), None);
        map.insert(i, true);
        assert_eq!(map.get(i), Some(true));
        map.insert(i, false);
        assert_eq!(map.get(i), Some(false));
        map.remove(i);
        assert_eq!(map.get(i), None);
      }
      for i in 0..i {
        assert_eq!(map.get(i), None);
        map.insert(i, true);
        assert_eq!(map.get(i), Some(true));
      }
      for i in 0..i {
        assert_eq!(map.get(i), Some(true));
        map.insert(i, false);
        assert_eq!(map.get(i), Some(false));
      }
    }
  }
}

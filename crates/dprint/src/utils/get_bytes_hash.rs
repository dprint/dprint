use std::hash::Hasher;

pub fn get_bytes_hash(bytes: &[u8]) -> u64 {
  let mut hasher = FastInsecureHasher::default();
  hasher.write(bytes);
  hasher.finish()
}

/// Hashes multiple byte slices in order, producing a single combined hash.
/// Useful when a cached resource's identity depends on more than one file.
pub fn get_combined_bytes_hash(parts: &[&[u8]]) -> u64 {
  let mut hasher = FastInsecureHasher::default();
  for part in parts {
    hasher.write(part);
  }
  hasher.finish()
}

/// A very fast insecure hasher that uses the xxHash algorithm.
#[derive(Default)]
pub struct FastInsecureHasher(twox_hash::XxHash64);

impl FastInsecureHasher {
  pub fn finish(&self) -> u64 {
    self.0.finish()
  }
}

impl Hasher for FastInsecureHasher {
  fn finish(&self) -> u64 {
    self.0.finish()
  }

  fn write(&mut self, bytes: &[u8]) {
    self.0.write(bytes)
  }
}

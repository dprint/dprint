use twox_hash::XxHash64;
use std::hash::Hasher;

pub fn get_bytes_hash(bytes: &[u8]) -> u64 {
    let mut hasher = XxHash64::default();
    hasher.write(bytes);
    hasher.finish()
}

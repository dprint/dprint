use dprint_core::types::ErrBox;

pub fn get_bytes_hash(bytes: &[u8]) -> u64 {
    use std::hash::Hasher;
    use twox_hash::XxHash64;

    let mut hasher = XxHash64::default();
    hasher.write(bytes);
    hasher.finish()
}

pub fn get_sha256_checksum(bytes: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn verify_sha256_checksum(bytes: &[u8], checksum: &str) -> Result<(), ErrBox> {
    let bytes_checksum = get_sha256_checksum(bytes);
    if bytes_checksum != checksum {
        err!("The checksum {} did not match the expected checksum of {}.", bytes_checksum, checksum)
    } else {
        Ok(())
    }
}

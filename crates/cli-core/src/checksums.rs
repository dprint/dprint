use crate::types::ErrBox;

pub fn get_sha256_checksum(bytes: &[u8]) -> String {
  use sha2::{Digest, Sha256};
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

#[derive(Clone)]
pub struct ChecksumPathOrUrl {
  pub path_or_url: String,
  pub checksum: Option<String>,
}

pub fn parse_checksum_path_or_url(text: &str) -> ChecksumPathOrUrl {
  // todo: this should tell whether what follows the '@' symbol is a checksum
  match text.rfind('@') {
    Some(index) => ChecksumPathOrUrl {
      path_or_url: text[..index].to_string(),
      checksum: Some(text[index + 1..].to_string()),
    },
    None => ChecksumPathOrUrl {
      path_or_url: text.to_string(),
      checksum: None,
    },
  }
}

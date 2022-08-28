use anyhow::bail;
use anyhow::Result;

pub fn get_sha256_checksum(bytes: &[u8]) -> String {
  use sha2::Digest;
  use sha2::Sha256;
  let mut hasher = Sha256::new();
  hasher.update(bytes);
  format!("{:x}", hasher.finalize())
}

pub fn verify_sha256_checksum(bytes: &[u8], checksum: &str) -> Result<()> {
  let bytes_checksum = get_sha256_checksum(bytes);
  if bytes_checksum != checksum {
    bail!(
      "The checksum did not match the expected checksum.\n\nActual: {}\nExpected: {}",
      bytes_checksum,
      checksum
    )
  } else {
    Ok(())
  }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChecksumPathOrUrl {
  pub path_or_url: String,
  pub checksum: Option<String>,
}

pub fn parse_checksum_path_or_url(text: &str) -> ChecksumPathOrUrl {
  if let Some(index) = text.rfind('@') {
    let path_or_url = text[..index].to_string();
    if path_or_url.ends_with(".wasm") || path_or_url.ends_with(".json") {
      return ChecksumPathOrUrl {
        path_or_url,
        checksum: Some(text[index + 1..].to_string()),
      };
    }
  }

  ChecksumPathOrUrl {
    path_or_url: text.to_string(),
    checksum: None,
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  pub fn parses_checksum_path_or_url() {
    assert_eq!(
      parse_checksum_path_or_url("./test/@test/test.wasm"),
      ChecksumPathOrUrl {
        path_or_url: "./test/@test/test.wasm".to_string(),
        checksum: None,
      }
    );
    assert_eq!(
      parse_checksum_path_or_url("./test/test.wasm@ca9a97de84cbb2cd60534eb72c0455f3ca8704743917569ace70499136cf5c9c"),
      ChecksumPathOrUrl {
        path_or_url: "./test/test.wasm".to_string(),
        checksum: Some("ca9a97de84cbb2cd60534eb72c0455f3ca8704743917569ace70499136cf5c9c".to_string()),
      }
    );
  }
}

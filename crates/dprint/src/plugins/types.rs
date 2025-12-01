use std::fmt;

use anyhow::Result;
use dprint_core::plugins::PluginInfo;

use crate::environment::Environment;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::parse_checksum_path_or_url;
use crate::utils::resolve_url_or_file_path_to_path_source;

#[derive(Clone)]
pub struct CompilationResult {
  pub bytes: Vec<u8>,
  pub plugin_info: PluginInfo,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PluginSourceReference {
  pub path_source: PathSource,
  pub checksum: Option<String>,
}

impl PluginSourceReference {
  /// Gets the source for display purposes without a checksum.
  pub fn display(&self) -> String {
    self.path_source.display()
  }

  pub fn plugin_kind(&self) -> Option<PluginKind> {
    self.path_source.plugin_kind()
  }

  pub fn without_checksum(&self) -> PluginSourceReference {
    PluginSourceReference {
      path_source: self.path_source.clone(),
      checksum: None,
    }
  }

  pub fn to_full_string(&self) -> String {
    if let Some(checksum) = &self.checksum {
      format!("{}@{}", self.path_source, checksum)
    } else {
      self.path_source.display()
    }
  }

  #[cfg(test)]
  pub fn new_local(path: impl AsRef<std::path::Path>) -> PluginSourceReference {
    use crate::environment::CanonicalizedPathBuf;

    PluginSourceReference {
      path_source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing(path)),
      checksum: None,
    }
  }

  #[cfg(test)]
  pub fn new_remote_from_str(url: &str) -> PluginSourceReference {
    PluginSourceReference {
      path_source: PathSource::new_remote_from_str(url),
      checksum: None,
    }
  }
}

impl fmt::Display for PluginSourceReference {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{}", self.to_full_string())
  }
}

pub fn parse_plugin_source_reference(text: &str, base: &PathSource, environment: &impl Environment) -> Result<PluginSourceReference> {
  let checksum_reference = parse_checksum_path_or_url(text);
  let path_source = resolve_url_or_file_path_to_path_source(&checksum_reference.path_or_url, base, environment)?;

  Ok(PluginSourceReference {
    path_source,
    checksum: checksum_reference.checksum,
  })
}

#[cfg(test)]
mod tests {
  use crate::environment::CanonicalizedPathBuf;
  use crate::environment::TestEnvironment;

  use super::*;

  #[test]
  fn should_parse_plugin_without_checksum() {
    let environment = TestEnvironment::new();
    let result = parse_plugin_source_reference(
      "http://dprint.dev/wasm_plugin.wasm",
      &PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/")),
      &environment,
    )
    .unwrap();
    assert_eq!(
      result,
      PluginSourceReference {
        path_source: PathSource::new_remote_from_str("http://dprint.dev/wasm_plugin.wasm"),
        checksum: None,
      }
    );
  }

  #[test]
  fn should_parse_plugin_with_checksum() {
    let environment = TestEnvironment::new();
    let result = parse_plugin_source_reference(
      "http://dprint.dev/wasm_plugin.wasm@checksum",
      &PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/")),
      &environment,
    )
    .unwrap();
    assert_eq!(
      result,
      PluginSourceReference {
        path_source: PathSource::new_remote_from_str("http://dprint.dev/wasm_plugin.wasm"),
        checksum: Some(String::from("checksum")),
      }
    );
  }

  #[test]
  fn should_not_error_multiple_at_symbols() {
    let environment = TestEnvironment::new();
    let plugin_text = "http://dprint.dev/@other/wasm_plugin.wasm@checksum";
    let result = parse_plugin_source_reference(&plugin_text, &PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/")), &environment).unwrap();
    assert_eq!(
      result,
      PluginSourceReference {
        path_source: PathSource::new_remote_from_str("http://dprint.dev/@other/wasm_plugin.wasm"),
        checksum: Some(String::from("checksum")),
      }
    );
  }

  #[test]
  fn should_parse_non_wasm_plugin_with_checksum() {
    let environment = TestEnvironment::new();
    let result = parse_plugin_source_reference(
      "http://dprint.dev/plugin.json@checksum",
      &PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/")),
      &environment,
    )
    .unwrap();
    assert_eq!(
      result,
      PluginSourceReference {
        path_source: PathSource::new_remote_from_str("http://dprint.dev/plugin.json"),
        checksum: Some(String::from("checksum")),
      }
    );
  }

  #[test]
  fn should_not_error_for_non_wasm_plugin_no_checksum() {
    // this now errors at a higher level when verifying the checksum instead
    let environment = TestEnvironment::new();
    let result = parse_plugin_source_reference(
      "http://dprint.dev/plugin.json",
      &PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/")),
      &environment,
    )
    .unwrap();
    assert_eq!(
      result,
      PluginSourceReference {
        path_source: PathSource::new_remote_from_str("http://dprint.dev/plugin.json"),
        checksum: None,
      }
    );
  }
}

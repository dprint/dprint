use anyhow::Result;
use anyhow::bail;

use super::PluginKind;

/// The filename within the npm package that contains the plugin.
const DEFAULT_NPM_PLUGIN_FILE: &str = "plugin.wasm";

/// A parsed npm package reference from a plugin string like
/// `npm:@scope/name@version/plugin.json`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NpmSpecifier {
  /// The full package name (e.g., "@dprint/typescript" or "dprint-plugin-foo").
  pub name: String,
  /// The exact version, if specified (e.g., "0.23.0").
  /// When None, the plugin should be resolved via node_modules.
  pub version: Option<String>,
  /// The plugin filename within the package (e.g., "plugin.wasm" or "plugin.json").
  pub path: String,
}

/// The result of parsing an npm plugin string, separating the specifier from the checksum.
pub struct ParsedNpmSpecifier {
  pub specifier: NpmSpecifier,
  /// The checksum of the npm tarball, if specified.
  pub checksum: Option<String>,
}

impl NpmSpecifier {
  pub fn plugin_kind(&self) -> PluginKind {
    if self.path.ends_with(".json") {
      PluginKind::Process
    } else {
      PluginKind::Wasm
    }
  }

  /// Returns the specifier string suitable for display.
  pub fn display(&self) -> String {
    let path_suffix = if self.path == DEFAULT_NPM_PLUGIN_FILE {
      String::new()
    } else {
      format!("/{}", self.path)
    };
    match &self.version {
      Some(version) => format!("npm:{}@{}{}", self.name, version, path_suffix),
      None => format!("npm:{}{}", self.name, path_suffix),
    }
  }
}

/// Parses an npm specifier string. Supported formats:
/// - `npm:@scope/name` (node_modules, wasm)
/// - `npm:@scope/name@version` (registry, wasm)
/// - `npm:@scope/name@version/plugin.json` (registry, process)
/// - `npm:@scope/name@version/plugin.json@checksum` (registry, process, with checksum)
/// - `npm:@scope/name@version/plugin.wasm` (registry, explicit wasm)
pub fn parse_npm_specifier(text: &str) -> Result<ParsedNpmSpecifier> {
  let Some(rest) = text.strip_prefix("npm:") else {
    bail!("Expected npm specifier to start with 'npm:': {}", text);
  };

  if rest.is_empty() {
    bail!("Expected a package name after 'npm:': {}", text);
  }

  let (name, after_name) = parse_package_name(rest, text)?;

  if after_name.is_empty() {
    return Ok(ParsedNpmSpecifier {
      specifier: NpmSpecifier {
        name,
        version: None,
        path: DEFAULT_NPM_PLUGIN_FILE.to_string(),
      },
      checksum: None,
    });
  }

  // after_name starts with '@' (version separator) or '/' (path)
  if after_name.starts_with('/') {
    // no version, just a path: npm:@scope/name/plugin.json
    let (path, checksum) = parse_path_and_checksum(&after_name[1..], text)?;
    return Ok(ParsedNpmSpecifier {
      specifier: NpmSpecifier { name, version: None, path },
      checksum,
    });
  }

  // after_name starts with '@'
  let after_at = &after_name[1..];

  // split version from the rest — version ends at '/' (path) or '@' (checksum) or end
  let (version, remainder) = split_version(after_at);
  if version.is_empty() {
    bail!("Expected a version after '@' in npm specifier: {}", text);
  }

  if remainder.is_empty() {
    return Ok(ParsedNpmSpecifier {
      specifier: NpmSpecifier {
        name,
        version: Some(version.to_string()),
        path: DEFAULT_NPM_PLUGIN_FILE.to_string(),
      },
      checksum: None,
    });
  }

  if remainder.starts_with('/') {
    // version followed by path: npm:@scope/name@version/plugin.json[@checksum]
    let (path, checksum) = parse_path_and_checksum(&remainder[1..], text)?;
    return Ok(ParsedNpmSpecifier {
      specifier: NpmSpecifier {
        name,
        version: Some(version.to_string()),
        path,
      },
      checksum,
    });
  }

  if remainder.starts_with('@') {
    // version followed by checksum (no path): npm:@scope/name@version@checksum
    let checksum = &remainder[1..];
    if checksum.is_empty() {
      bail!("Expected a checksum after '@' in npm specifier: {}", text);
    }
    return Ok(ParsedNpmSpecifier {
      specifier: NpmSpecifier {
        name,
        version: Some(version.to_string()),
        path: DEFAULT_NPM_PLUGIN_FILE.to_string(),
      },
      checksum: Some(checksum.to_string()),
    });
  }

  bail!("Unexpected characters after version in npm specifier: {}", text);
}

/// Splits a version string from the remainder. Version ends at '/' or '@'.
fn split_version(s: &str) -> (&str, &str) {
  for (i, c) in s.char_indices() {
    if c == '/' || c == '@' {
      return (&s[..i], &s[i..]);
    }
  }
  (s, "")
}

/// Parses `plugin.json@checksum` or `plugin.wasm` from the path portion.
fn parse_path_and_checksum(s: &str, original: &str) -> Result<(String, Option<String>)> {
  if s.is_empty() {
    bail!("Expected a plugin filename after '/' in npm specifier: {}", original);
  }
  if let Some(at_idx) = s.find('@') {
    let path = &s[..at_idx];
    let checksum = &s[at_idx + 1..];
    if path.is_empty() {
      bail!("Expected a plugin filename after '/' in npm specifier: {}", original);
    }
    if checksum.is_empty() {
      bail!("Expected a checksum after '@' in npm specifier: {}", original);
    }
    Ok((path.to_string(), Some(checksum.to_string())))
  } else {
    Ok((s.to_string(), None))
  }
}

/// Parses the package name from the remainder after `npm:`.
/// Returns (name, rest_after_name) where rest_after_name starts with '@', '/', or is empty.
fn parse_package_name<'a>(rest: &'a str, original: &str) -> Result<(String, &'a str)> {
  if rest.starts_with('@') {
    // scoped package: @scope/name
    let Some(slash_idx) = rest.find('/') else {
      bail!("Expected '/' after scope in npm specifier: {}", original);
    };
    let after_slash = &rest[slash_idx + 1..];
    if after_slash.is_empty() {
      bail!("Expected a package name after '/' in npm specifier: {}", original);
    }
    // the name ends at the next '@' or '/' or end of string
    // for scoped packages, the second '/' is the path separator
    match after_slash.find(|c| c == '@' || c == '/') {
      Some(idx) => {
        let name_end = slash_idx + 1 + idx;
        Ok((rest[..name_end].to_string(), &rest[name_end..]))
      }
      None => Ok((rest.to_string(), "")),
    }
  } else {
    // unscoped package: name ends at '@' or '/'
    match rest.find(|c| c == '@' || c == '/') {
      Some(idx) => Ok((rest[..idx].to_string(), &rest[idx..])),
      None => Ok((rest.to_string(), "")),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_scoped_with_version() {
    let result = parse_npm_specifier("npm:@dprint/typescript@0.23.0").unwrap();
    assert_eq!(result.specifier.name, "@dprint/typescript");
    assert_eq!(result.specifier.version, Some("0.23.0".to_string()));
    assert_eq!(result.specifier.path, "plugin.wasm");
    assert_eq!(result.specifier.plugin_kind(), PluginKind::Wasm);
    assert_eq!(result.checksum, None);
  }

  #[test]
  fn parse_scoped_with_version_and_checksum() {
    let result = parse_npm_specifier("npm:@dprint/prettier@0.23.0@abc123def").unwrap();
    assert_eq!(result.specifier.name, "@dprint/prettier");
    assert_eq!(result.specifier.version, Some("0.23.0".to_string()));
    assert_eq!(result.specifier.path, "plugin.wasm");
    assert_eq!(result.checksum, Some("abc123def".to_string()));
  }

  #[test]
  fn parse_scoped_without_version() {
    let result = parse_npm_specifier("npm:@dprint/typescript").unwrap();
    assert_eq!(result.specifier.name, "@dprint/typescript");
    assert_eq!(result.specifier.version, None);
    assert_eq!(result.specifier.path, "plugin.wasm");
    assert_eq!(result.checksum, None);
  }

  #[test]
  fn parse_with_plugin_json_path() {
    let result = parse_npm_specifier("npm:@dprint/prettier@0.23.0/plugin.json").unwrap();
    assert_eq!(result.specifier.name, "@dprint/prettier");
    assert_eq!(result.specifier.version, Some("0.23.0".to_string()));
    assert_eq!(result.specifier.path, "plugin.json");
    assert_eq!(result.specifier.plugin_kind(), PluginKind::Process);
    assert_eq!(result.checksum, None);
  }

  #[test]
  fn parse_with_plugin_json_path_and_checksum() {
    let result = parse_npm_specifier("npm:@dprint/prettier@0.23.0/plugin.json@abc123").unwrap();
    assert_eq!(result.specifier.name, "@dprint/prettier");
    assert_eq!(result.specifier.version, Some("0.23.0".to_string()));
    assert_eq!(result.specifier.path, "plugin.json");
    assert_eq!(result.checksum, Some("abc123".to_string()));
  }

  #[test]
  fn parse_with_explicit_plugin_wasm() {
    let result = parse_npm_specifier("npm:@dprint/typescript@0.23.0/plugin.wasm").unwrap();
    assert_eq!(result.specifier.name, "@dprint/typescript");
    assert_eq!(result.specifier.version, Some("0.23.0".to_string()));
    assert_eq!(result.specifier.path, "plugin.wasm");
    assert_eq!(result.specifier.plugin_kind(), PluginKind::Wasm);
    assert_eq!(result.checksum, None);
  }

  #[test]
  fn parse_no_version_with_path() {
    let result = parse_npm_specifier("npm:@dprint/prettier/plugin.json").unwrap();
    assert_eq!(result.specifier.name, "@dprint/prettier");
    assert_eq!(result.specifier.version, None);
    assert_eq!(result.specifier.path, "plugin.json");
    assert_eq!(result.specifier.plugin_kind(), PluginKind::Process);
  }

  #[test]
  fn parse_unscoped_with_version() {
    let result = parse_npm_specifier("npm:dprint-plugin-foo@1.0.0").unwrap();
    assert_eq!(result.specifier.name, "dprint-plugin-foo");
    assert_eq!(result.specifier.version, Some("1.0.0".to_string()));
    assert_eq!(result.specifier.path, "plugin.wasm");
    assert_eq!(result.checksum, None);
  }

  #[test]
  fn parse_unscoped_without_version() {
    let result = parse_npm_specifier("npm:dprint-plugin-foo").unwrap();
    assert_eq!(result.specifier.name, "dprint-plugin-foo");
    assert_eq!(result.specifier.version, None);
    assert_eq!(result.checksum, None);
  }

  #[test]
  fn parse_unscoped_with_path() {
    let result = parse_npm_specifier("npm:dprint-plugin-foo@1.0.0/plugin.json@sha256hash").unwrap();
    assert_eq!(result.specifier.name, "dprint-plugin-foo");
    assert_eq!(result.specifier.version, Some("1.0.0".to_string()));
    assert_eq!(result.specifier.path, "plugin.json");
    assert_eq!(result.checksum, Some("sha256hash".to_string()));
  }

  #[test]
  fn display_default_path_omitted() {
    let result = parse_npm_specifier("npm:@dprint/typescript@0.23.0").unwrap();
    assert_eq!(result.specifier.display(), "npm:@dprint/typescript@0.23.0");
  }

  #[test]
  fn display_explicit_json_path() {
    let result = parse_npm_specifier("npm:@dprint/prettier@0.23.0/plugin.json").unwrap();
    assert_eq!(result.specifier.display(), "npm:@dprint/prettier@0.23.0/plugin.json");
  }

  #[test]
  fn display_without_version() {
    let result = parse_npm_specifier("npm:@dprint/typescript").unwrap();
    assert_eq!(result.specifier.display(), "npm:@dprint/typescript");
  }

  #[test]
  fn display_without_version_with_path() {
    let result = parse_npm_specifier("npm:@dprint/prettier/plugin.json").unwrap();
    assert_eq!(result.specifier.display(), "npm:@dprint/prettier/plugin.json");
  }

  #[test]
  fn error_no_prefix() {
    assert!(parse_npm_specifier("@dprint/typescript@0.23.0").is_err());
  }

  #[test]
  fn error_empty_after_npm() {
    assert!(parse_npm_specifier("npm:").is_err());
  }

  #[test]
  fn error_no_slash_in_scope() {
    assert!(parse_npm_specifier("npm:@dprint").is_err());
  }

  #[test]
  fn error_empty_version() {
    assert!(parse_npm_specifier("npm:@dprint/typescript@").is_err());
  }

  #[test]
  fn error_empty_checksum() {
    assert!(parse_npm_specifier("npm:@dprint/typescript@0.23.0@").is_err());
  }

  #[test]
  fn error_empty_path() {
    assert!(parse_npm_specifier("npm:@dprint/typescript@0.23.0/").is_err());
  }

  #[test]
  fn error_empty_checksum_after_path() {
    assert!(parse_npm_specifier("npm:@dprint/typescript@0.23.0/plugin.json@").is_err());
  }
}

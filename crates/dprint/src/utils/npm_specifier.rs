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
  /// Returns the plugin kind implied by `path`'s extension (case-insensitive).
  /// Defaults to `Wasm` for any other extension — callers that need to reject
  /// non-plugin paths should run `validate_plugin_extension` after parsing.
  pub fn plugin_kind(&self) -> PluginKind {
    plugin_kind_from_extension(&self.path).unwrap_or(PluginKind::Wasm)
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
  if let Some(rest) = after_name.strip_prefix('/') {
    // no version, just a path: npm:@scope/name/plugin.json
    let (path, checksum) = parse_path_and_checksum(rest, text)?;
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

  if let Some(rest) = remainder.strip_prefix('/') {
    // version followed by path: npm:@scope/name@version/plugin.json[@checksum]
    let (path, checksum) = parse_path_and_checksum(rest, text)?;
    return Ok(ParsedNpmSpecifier {
      specifier: NpmSpecifier {
        name,
        version: Some(version.to_string()),
        path,
      },
      checksum,
    });
  }

  if let Some(checksum) = remainder.strip_prefix('@') {
    // version followed by checksum (no path): npm:@scope/name@version@checksum
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
/// Note: this does not validate the extension, since npm specifiers also
/// appear inside a process plugin's plugin.json (referencing the executable
/// file in a per-platform npm package — typically a bare binary name).
/// Callers that need a plugin file specifically validate the extension
/// themselves via `validate_plugin_extension`.
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

/// Rejects an npm specifier whose path doesn't end in `.wasm` or `.json`
/// (case-insensitive). Call this on a parsed `NpmSpecifier` when the
/// specifier is supposed to identify a top-level plugin (as opposed to an
/// embedded executable-binary reference inside a process plugin manifest).
pub fn validate_plugin_extension(specifier: &NpmSpecifier, original: &str) -> Result<()> {
  if plugin_kind_from_extension(&specifier.path).is_none() {
    bail!(
      "Unsupported plugin file extension in npm specifier '{}': '{}'. Expected '.wasm' or '.json'.",
      original,
      specifier.path,
    );
  }
  Ok(())
}

/// Maps a path's extension (case-insensitive) to a plugin kind, or `None`
/// if the extension isn't one we recognize. Shared by
/// `validate_plugin_extension` and `NpmSpecifier::plugin_kind`.
fn plugin_kind_from_extension(path: &str) -> Option<PluginKind> {
  let (_, ext) = path.rsplit_once('.')?;
  if ext.eq_ignore_ascii_case("wasm") {
    Some(PluginKind::Wasm)
  } else if ext.eq_ignore_ascii_case("json") {
    Some(PluginKind::Process)
  } else {
    None
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
    match after_slash.find(['@', '/']) {
      Some(idx) => {
        let name_end = slash_idx + 1 + idx;
        Ok((rest[..name_end].to_string(), &rest[name_end..]))
      }
      None => Ok((rest.to_string(), "")),
    }
  } else {
    // unscoped package: name ends at '@' or '/'
    match rest.find(['@', '/']) {
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

  #[test]
  fn validate_plugin_extension_rejects_non_plugin_paths() {
    // local/remote plugin sources only ever classify .wasm/.json. Top-level
    // npm plugin specifiers should be rejected the same way — but parsing
    // itself stays permissive because npm: specifiers are also used inside
    // process plugin manifests to reference the per-platform executable
    // binary (typically a bare filename like `foo` or `foo.exe`).
    let parsed = parse_npm_specifier("npm:foo@1.0.0/plugin.txt").unwrap();
    let err = validate_plugin_extension(&parsed.specifier, "npm:foo@1.0.0/plugin.txt").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Unsupported plugin file extension"), "got: {msg}");
    assert!(msg.contains("plugin.txt"), "got: {msg}");

    // bare names with no extension are also rejected
    let parsed = parse_npm_specifier("npm:foo@1.0.0/plugin").unwrap();
    assert!(validate_plugin_extension(&parsed.specifier, "npm:foo@1.0.0/plugin").is_err());
  }

  #[test]
  fn validate_plugin_extension_accepts_arbitrary_paths_only_through_parse_not_validate() {
    // npm specifiers referencing the per-platform executable file (from
    // inside a process plugin manifest) carry an arbitrary path — typically
    // a bare binary name like `foo` or `foo.exe`. Parsing must accept those,
    // but plugin-extension validation should still reject them at the
    // top-level plugin source position.
    let parsed = parse_npm_specifier("npm:foo-bin@1.0.0/foo").unwrap();
    assert_eq!(parsed.specifier.path, "foo");
    assert!(validate_plugin_extension(&parsed.specifier, "npm:foo-bin@1.0.0/foo").is_err());

    let parsed = parse_npm_specifier("npm:foo-bin@1.0.0/foo.exe").unwrap();
    assert_eq!(parsed.specifier.path, "foo.exe");
    assert!(validate_plugin_extension(&parsed.specifier, "npm:foo-bin@1.0.0/foo.exe").is_err());
  }

  #[test]
  fn parse_extension_case_insensitive() {
    // matches PathSource::plugin_kind's case-insensitive behavior for
    // local/remote sources.
    let result = parse_npm_specifier("npm:foo@1.0.0/plugin.JSON").unwrap();
    assert_eq!(result.specifier.path, "plugin.JSON");
    assert_eq!(result.specifier.plugin_kind(), PluginKind::Process);
    assert!(validate_plugin_extension(&result.specifier, "npm:foo@1.0.0/plugin.JSON").is_ok());

    let result = parse_npm_specifier("npm:foo@1.0.0/PLUGIN.WASM").unwrap();
    assert_eq!(result.specifier.path, "PLUGIN.WASM");
    assert_eq!(result.specifier.plugin_kind(), PluginKind::Wasm);
    assert!(validate_plugin_extension(&result.specifier, "npm:foo@1.0.0/PLUGIN.WASM").is_ok());
  }
}

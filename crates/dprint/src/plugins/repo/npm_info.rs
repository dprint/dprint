use anyhow::Result;
use jsonc_parser::JsonObject;

use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::plugins::FetchNpmLatestInfo;
use crate::plugins::PluginSourceReference;
use crate::plugins::fetch_npm_latest_info;
use crate::utils::NpmSpecifier;
use crate::utils::PathSource;
use crate::utils::PluginKind;

/// The file within an npm package that holds a process plugin's manifest.
const NPM_PROCESS_PLUGIN_FILE: &str = "plugin.json";
/// The file within an npm package that holds a wasm plugin.
const NPM_WASM_PLUGIN_FILE: &str = "plugin.wasm";

/// The npm package a plugin is distributed as — the `npm` property found on a
/// plugin in the registry's info.json and in a plugin's latest.json.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PluginNpmInfo {
  /// The npm package name (ex. `@dprint/json`).
  pub name: String,
}

/// Inputs to [`PluginNpmInfo::resolve_latest`].
#[derive(Clone)]
pub struct ResolveNpmLatestOptions {
  /// Compute the package's checksum even for a wasm plugin, which is
  /// otherwise written without one (`dprint add --checksum`).
  pub force_checksum: bool,
  /// The directory of the config file the plugin is being written to. The
  /// registry (`.npmrc`) is resolved from it, and it's recorded on the
  /// resulting reference for resolving the package later.
  pub base_dir: Option<CanonicalizedPathBuf>,
}

impl PluginNpmInfo {
  /// Parses the registry's `npm` property, ignoring one without a package
  /// name since it says nothing about where to get the plugin.
  pub fn parse(mut obj: JsonObject) -> Option<PluginNpmInfo> {
    let name = obj.take_string("name")?.into_owned();
    if name.is_empty() {
      return None;
    }
    Some(PluginNpmInfo { name })
  }

  /// Resolves the package's latest release from the npm registry.
  ///
  /// The version comes from the registry rather than from the info file or
  /// latest.json that named the package, since those are published separately
  /// and can lag behind what's actually on npm.
  ///
  /// `plugin_kind` says whether the package ships a wasm or a process plugin,
  /// which decides the file within it and whether a checksum is required.
  pub async fn resolve_latest(&self, plugin_kind: PluginKind, options: ResolveNpmLatestOptions, environment: &impl Environment) -> Result<ResolvedNpmPlugin> {
    let ResolveNpmLatestOptions { force_checksum, base_dir } = options;
    let unversioned = NpmSpecifier {
      name: self.name.clone(),
      version: None,
      path: match plugin_kind {
        PluginKind::Wasm => NPM_WASM_PLUGIN_FILE.to_string(),
        PluginKind::Process => NPM_PROCESS_PLUGIN_FILE.to_string(),
      },
    };
    // a process plugin's checksum is fetched regardless of `force_checksum` —
    // it can't be resolved without one
    let latest = fetch_npm_latest_info(
      FetchNpmLatestInfo {
        specifier: &unversioned,
        start_dir: base_dir.as_ref().map(|dir| dir.as_ref()),
        want_tarball_sha: force_checksum,
      },
      environment,
    )
    .await?;
    Ok(ResolvedNpmPlugin {
      reference: PluginSourceReference {
        path_source: PathSource::new_npm(
          NpmSpecifier {
            version: Some(latest.version.clone()),
            ..unversioned
          },
          base_dir,
        ),
        checksum: latest.tarball_sha256,
      },
      version: latest.version,
    })
  }
}

/// A plugin's npm package as resolved from the registry.
pub struct ResolvedNpmPlugin {
  /// The latest version published to the registry. Always the version of
  /// `reference`'s specifier, kept here so callers don't have to dig it out.
  pub version: String,
  reference: PluginSourceReference,
}

impl ResolvedNpmPlugin {
  /// The text to write into a config file's `plugins` array.
  pub fn config_file_entry(&self) -> String {
    self.reference.to_full_string()
  }

  /// The plugin's source reference, for pointing an existing config file entry
  /// at it.
  pub fn as_source_reference(&self) -> PluginSourceReference {
    self.reference.clone()
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;
  use jsonc_parser::JsonValue;
  use jsonc_parser::parse_to_value;
  use pretty_assertions::assert_eq;

  fn parse(text: &str) -> Option<PluginNpmInfo> {
    match parse_to_value(text, &Default::default()) {
      Ok(Some(JsonValue::Object(obj))) => PluginNpmInfo::parse(obj),
      _ => panic!("expected an object"),
    }
  }

  #[test]
  fn parses_package_name() {
    assert_eq!(
      parse(r#"{ "name": "@dprint/json" }"#),
      Some(PluginNpmInfo {
        name: "@dprint/json".to_string()
      })
    );
    // a package name is the one thing we can't do without
    assert_eq!(parse(r#"{ "checksum": "abc" }"#), None);
    assert_eq!(parse(r#"{ "name": "" }"#), None);
  }

  #[test]
  fn resolves_the_latest_version_from_the_registry() {
    let environment = TestEnvironment::new();
    let tarball = crate::test_helpers::create_test_npm_tarball(&[("package/plugin.wasm", crate::test_helpers::WASM_PLUGIN_BYTES)]);
    let tarball_checksum = crate::utils::get_sha256_checksum(&tarball);
    environment.add_remote_file_bytes(
      "https://registry.npmjs.org/@dprint/json",
      serde_json::json!({
        "dist-tags": { "latest": "1.2.3" },
        "versions": { "1.2.3": { "dist": { "tarball": "https://registry.npmjs.org/@dprint/json/-/json-1.2.3.tgz" } } }
      })
      .to_string()
      .into_bytes(),
    );
    environment.add_remote_file_bytes("https://registry.npmjs.org/@dprint/json/-/json-1.2.3.tgz", tarball);
    let npm = PluginNpmInfo {
      name: "@dprint/json".to_string(),
    };

    environment.clone().run_in_runtime(async move {
      let options = |force_checksum| ResolveNpmLatestOptions {
        force_checksum,
        base_dir: None,
      };

      // a wasm plugin is written without a checksum...
      let resolved = npm.resolve_latest(PluginKind::Wasm, options(false), &environment).await.unwrap();
      assert_eq!(resolved.version, "1.2.3");
      assert_eq!(resolved.config_file_entry(), "npm:@dprint/json@1.2.3");
      // ...unless one was asked for
      let resolved = npm.resolve_latest(PluginKind::Wasm, options(true), &environment).await.unwrap();
      assert_eq!(resolved.config_file_entry(), format!("npm:@dprint/json@1.2.3@{}", tarball_checksum));
      // a process plugin always carries the package's checksum
      let resolved = npm.resolve_latest(PluginKind::Process, options(false), &environment).await.unwrap();
      assert_eq!(resolved.config_file_entry(), format!("npm:@dprint/json@1.2.3/plugin.json@{}", tarball_checksum));
      assert_eq!(resolved.as_source_reference().to_full_string(), resolved.config_file_entry());
    });
  }
}

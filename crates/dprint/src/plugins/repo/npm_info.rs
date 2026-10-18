use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use jsonc_parser::JsonObject;

use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::plugins::FetchNpmLatestInfo;
use crate::plugins::PluginSourceReference;
use crate::plugins::fetch_npm_latest_info;
use crate::utils::DependencyAgeCutoff;
use crate::utils::NpmSpecifier;
use crate::utils::PathSource;
use crate::utils::PluginKind;
use crate::utils::parse_npm_specifier;
use crate::utils::validate_plugin_extension;
use crate::utils::validate_safe_version;

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
  /// Where the plugin sits within the package, for one that doesn't ship it at
  /// the root (ex. `json/plugin.wasm` in a package holding several plugins).
  /// Defaults to `plugin.wasm` / `plugin.json` by plugin kind.
  pub path: Option<String>,
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
  /// How old the resolved version must be, when the user asked for a minimum
  /// dependency age. `None` selects whatever the registry tags as latest.
  pub minimum_dependency_age: Option<DependencyAgeCutoff>,
}

impl PluginNpmInfo {
  /// Parses the registry's `npm` property, ignoring one without a package
  /// name since it says nothing about where to get the plugin.
  pub fn parse(mut obj: JsonObject) -> Option<PluginNpmInfo> {
    let name = obj.take_string("name")?.into_owned();
    if name.is_empty() {
      return None;
    }
    Some(PluginNpmInfo {
      name,
      path: obj.take_string("path").map(|path| path.into_owned()).filter(|path| !path.is_empty()),
    })
  }

  /// Resolves the package's latest release from the npm registry.
  ///
  /// The version comes from the registry rather than from the info file or
  /// latest.json that named the package, since those are published separately
  /// and can lag behind what's actually on npm.
  ///
  /// `plugin_kind` says whether the package ships a wasm or a process plugin,
  /// which decides the file within it and whether a checksum is required. A
  /// package that declares its own `path` decides both from that instead.
  pub async fn resolve_latest(&self, environment: &impl Environment, plugin_kind: PluginKind, options: ResolveNpmLatestOptions) -> Result<ResolvedNpmPlugin> {
    let ResolveNpmLatestOptions {
      force_checksum,
      base_dir,
      minimum_dependency_age,
    } = options;
    let unversioned = self.unversioned_specifier(plugin_kind)?;
    // a process plugin's checksum is fetched regardless of `force_checksum` —
    // it can't be resolved without one
    let latest = fetch_npm_latest_info(
      FetchNpmLatestInfo {
        specifier: &unversioned,
        start_dir: base_dir.as_ref().map(|dir| dir.as_ref()),
        want_tarball_sha: force_checksum,
        minimum_dependency_age: minimum_dependency_age.as_ref(),
      },
      environment,
    )
    .await?;
    // the registry's `latest` tag hasn't been through the specifier parser, so
    // it gets the same check a user-written version does. a version holding a
    // '/' would otherwise render into the config as a path and come back as a
    // process plugin — a native binary in place of the wasm this resolved as
    validate_safe_version(&latest.version, &format!("npm:{}@{}", self.name, latest.version))?;
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

  /// The package's specifier without a version yet.
  ///
  /// The name and path come out of a registry file, so they go through the
  /// same parsing a user-written `npm:` specifier does: it rejects a path that
  /// would escape the package directory and one that doesn't name a plugin
  /// file, so neither reaches the filesystem or a config file unchecked.
  fn unversioned_specifier(&self, plugin_kind: PluginKind) -> Result<NpmSpecifier> {
    let path = self.path.clone().unwrap_or_else(|| {
      match plugin_kind {
        PluginKind::Wasm => NPM_WASM_PLUGIN_FILE,
        PluginKind::Process => NPM_PROCESS_PLUGIN_FILE,
      }
      .to_string()
    });
    let text = format!("npm:{}/{}", self.name, path);
    let parsed = parse_npm_specifier(&text).with_context(|| format!("Invalid npm package for plugin: {}", text))?;
    // the name has to survive parsing intact, otherwise it isn't a package name
    // at all (ex. one holding an '@' parses as a version)
    if parsed.specifier.name != self.name || parsed.specifier.version.is_some() {
      bail!("Invalid npm package name for plugin: '{}'", self.name);
    }
    validate_plugin_extension(&parsed.specifier, &text)?;
    Ok(parsed.specifier)
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
  fn parses_package_name_and_path() {
    assert_eq!(
      parse(r#"{ "name": "@dprint/json" }"#),
      Some(PluginNpmInfo {
        name: "@dprint/json".to_string(),
        path: None,
      })
    );
    assert_eq!(
      parse(r#"{ "name": "@dprint/example", "path": "json/plugin.wasm" }"#),
      Some(PluginNpmInfo {
        name: "@dprint/example".to_string(),
        path: Some("json/plugin.wasm".to_string()),
      })
    );
    // a package name is the one thing we can't do without
    assert_eq!(parse(r#"{ "path": "json/plugin.wasm" }"#), None);
    assert_eq!(parse(r#"{ "name": "" }"#), None);
    // an empty path is the same as none
    assert_eq!(parse(r#"{ "name": "a", "path": "" }"#).unwrap().path, None);
  }

  #[test]
  fn rejects_a_package_that_isnt_a_plugin_file_or_escapes_the_package() {
    let resolve = |name: &str, path: Option<&str>| {
      let npm = PluginNpmInfo {
        name: name.to_string(),
        path: path.map(ToString::to_string),
      };
      npm.unversioned_specifier(PluginKind::Wasm).err().map(|err| format!("{err:#}"))
    };

    // paths that would escape the extracted package
    assert!(resolve("pkg", Some("../evil.wasm")).unwrap().contains("must not contain '.' or '..' segments"));
    assert!(resolve("pkg", Some("/etc/evil.wasm")).unwrap().contains("must be relative"));
    assert!(resolve("pkg", Some("..\\evil.wasm")).unwrap().contains("must not contain backslashes"));
    // a file that isn't a plugin
    assert!(resolve("pkg", Some("readme.md")).unwrap().contains("Unsupported plugin file extension"));
    // a name that isn't a package name
    assert!(resolve("pkg@1.0.0", None).unwrap().contains("Invalid npm package name for plugin"));
    assert!(resolve("../pkg", None).unwrap().contains("must not contain '.' or '..' segments"));
    // ...and the shapes that are fine
    assert_eq!(resolve("@scope/pkg", Some("json/plugin.wasm")), None);
    assert_eq!(resolve("pkg", Some("nested/dir/plugin.json")), None);
    assert_eq!(resolve("@scope/pkg", None), None);
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
      path: None,
    };

    environment.clone().run_in_runtime(async move {
      let options = |force_checksum| ResolveNpmLatestOptions {
        minimum_dependency_age: None,
        force_checksum,
        base_dir: None,
      };

      // a wasm plugin is written without a checksum...
      let resolved = npm.resolve_latest(&environment, PluginKind::Wasm, options(false)).await.unwrap();
      assert_eq!(resolved.version, "1.2.3");
      assert_eq!(resolved.config_file_entry(), "npm:@dprint/json@1.2.3");
      // ...unless one was asked for
      let resolved = npm.resolve_latest(&environment, PluginKind::Wasm, options(true)).await.unwrap();
      assert_eq!(resolved.config_file_entry(), format!("npm:@dprint/json@1.2.3@{}", tarball_checksum));
      // a process plugin always carries the package's checksum
      let resolved = npm.resolve_latest(&environment, PluginKind::Process, options(false)).await.unwrap();
      assert_eq!(resolved.config_file_entry(), format!("npm:@dprint/json@1.2.3/plugin.json@{}", tarball_checksum));
      assert_eq!(resolved.as_source_reference().to_full_string(), resolved.config_file_entry());

      // a package that declares a path decides the plugin's kind from it,
      // whatever kind the registry file's own url implied
      let with_path = |path: &str| PluginNpmInfo {
        name: "@dprint/json".to_string(),
        path: Some(path.to_string()),
      };
      let resolved = with_path("json/plugin.wasm")
        .resolve_latest(&environment, PluginKind::Process, options(false))
        .await
        .unwrap();
      assert_eq!(resolved.config_file_entry(), "npm:@dprint/json@1.2.3/json/plugin.wasm");
      let resolved = with_path("exec/plugin.json")
        .resolve_latest(&environment, PluginKind::Wasm, options(false))
        .await
        .unwrap();
      assert_eq!(
        resolved.config_file_entry(),
        format!("npm:@dprint/json@1.2.3/exec/plugin.json@{}", tarball_checksum)
      );
    });
  }
}

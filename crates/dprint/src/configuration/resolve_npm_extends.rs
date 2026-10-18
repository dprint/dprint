use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;

use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::plugins::NpmrcChain;
use crate::plugins::ensure_npm_package_extracted;
use crate::plugins::find_package_in_node_modules;
use crate::utils::NpmPathSource;
use crate::utils::NpmSpecifier;
use crate::utils::PathSource;
use crate::utils::ResolvedFilePathWithBytes;
use crate::utils::parse_npm_specifier;

/// File names checked at the root of an npm package when it doesn't otherwise
/// say what its configuration file is.
const DEFAULT_CONFIG_FILE_NAMES: [&str; 2] = ["dprint.json", "dprint.jsonc"];

/// Resolves the npm specifiers in the `extends` of a configuration file and
/// of everything it extends.
///
/// Remembers what it reads so each `.npmrc`, package and package.json is only
/// looked at once for a configuration file no matter how many times it's used.
#[derive(Default)]
pub struct NpmExtendsResolver {
  /// The `.npmrc` files that apply to a directory.
  npmrc_chains: RefCell<HashMap<Option<PathBuf>, Rc<NpmrcChain>>>,
  /// Directories in the npm cache of the registry packages that were resolved.
  registry_package_dirs: RefCell<HashMap<RegistryPackageKey, PathBuf>>,
  package_jsons: RefCell<HashMap<PathBuf, Rc<Option<serde_json::Value>>>>,
}

#[derive(PartialEq, Eq, Hash)]
struct RegistryPackageKey {
  /// Directory the registry is resolved from.
  base_dir: Option<PathBuf>,
  name: String,
  version: String,
}

impl NpmExtendsResolver {
  /// Resolves an `npm:` specifier in a configuration file's `extends` property
  /// to the configuration file in the npm package. Supported formats:
  /// - `npm:@scope/name` (node_modules, the package's main configuration file)
  /// - `npm:@scope/name/sub/path` (node_modules, an export or file in the package)
  /// - `npm:@scope/name@version` (registry, the package's main configuration file)
  /// - `npm:@scope/name@version/sub/path` (registry, an export or file in the package)
  ///
  /// A configuration file found in node_modules is a local file the same as if
  /// it was referenced by its path, while one downloaded from the registry is
  /// considered remote configuration.
  pub async fn resolve(&self, text: &str, base: &PathSource, environment: &impl Environment) -> Result<ResolvedFilePathWithBytes> {
    let parsed = parse_npm_specifier(text)?;
    if parsed.checksum.is_some() {
      bail!("Checksums are not supported for npm specifiers in \"extends\": {}", text);
    }
    let name = parsed.specifier.name;
    let sub_path = parsed.path_was_explicit.then_some(parsed.specifier.path);

    match parsed.specifier.version {
      Some(version) => {
        let base_dir = match base {
          PathSource::Local(local) => Some(local.path.clone()),
          PathSource::Npm(npm) => npm.base_dir.clone(),
          PathSource::Remote(_) => None,
        };
        let package = self.ensure_registry_package(&name, &version, base_dir.as_ref(), environment).await?;
        let path = self.resolve_config_path_in_package(&package.dir, &name, sub_path.as_deref(), environment)?;
        let content = read_package_file(&package.dir, &name, &path, environment)?;
        Ok(ResolvedFilePathWithBytes {
          source: PathSource::new_npm(
            NpmSpecifier {
              name,
              version: Some(version),
              path,
            },
            base_dir,
          ),
          is_first_download: package.is_first_download,
          content,
        })
      }
      None => {
        let PathSource::Local(local_base) = base else {
          bail!(
            concat!(
              "Cannot resolve {} from node_modules because the configuration file extending it is not a local file ({}). ",
              "Specify a version to resolve it from the npm registry (ex. npm:{}@x.x.x)."
            ),
            text,
            base.display(),
            name,
          );
        };
        let Some(package_dir) = find_package_in_node_modules(&name, local_base.path.as_ref(), environment) else {
          bail!(
            concat!(
              "Could not find {} in node_modules. Make sure the package is installed (ex. npm install {}) ",
              "or specify a version to resolve it from the npm registry (ex. npm:{}@x.x.x)."
            ),
            name,
            name,
            name,
          );
        };
        let path = self.resolve_config_path_in_package(&package_dir, &name, sub_path.as_deref(), environment)?;
        let file_path = get_existing_package_file_path(&package_dir, &name, &path, environment)?;
        let file_path = environment.canonicalize(file_path)?;
        let content = environment.read_file_bytes(&file_path)?;
        Ok(ResolvedFilePathWithBytes {
          source: PathSource::new_local(file_path),
          is_first_download: false,
          content,
        })
      }
    }
  }

  /// Resolves a relative path in the `extends` property of a configuration file
  /// that came from an npm registry package to another file in that package.
  pub async fn resolve_relative(&self, relative_path: &str, base: &NpmPathSource, environment: &impl Environment) -> Result<ResolvedFilePathWithBytes> {
    let specifier = &base.specifier;
    let Some(version) = &specifier.version else {
      // configuration files from node_modules are local files
      bail!(
        "Cannot resolve a relative path against an npm specifier without a version: {}",
        specifier.display()
      );
    };
    let path = join_package_path(&specifier.path, relative_path)
      .with_context(|| format!("Failed resolving '{}' in the \"extends\" of {}", relative_path, specifier.display()))?;
    let package = self
      .ensure_registry_package(&specifier.name, version, base.base_dir.as_ref(), environment)
      .await?;
    let content = read_package_file(&package.dir, &specifier.name, &path, environment)?;
    Ok(ResolvedFilePathWithBytes {
      source: PathSource::new_npm(
        NpmSpecifier {
          name: specifier.name.clone(),
          version: Some(version.clone()),
          path,
        },
        base.base_dir.clone(),
      ),
      is_first_download: package.is_first_download,
      content,
    })
  }

  /// Gets the directory of a registry package in the npm cache, downloading
  /// the package when it's not there.
  async fn ensure_registry_package(
    &self,
    name: &str,
    version: &str,
    base_dir: Option<&CanonicalizedPathBuf>,
    environment: &impl Environment,
  ) -> Result<RegistryPackageDir> {
    let base_dir = base_dir.map(|d| -> &Path { d.as_ref() });
    let key = RegistryPackageKey {
      base_dir: base_dir.map(|d| d.to_path_buf()),
      name: name.to_string(),
      version: version.to_string(),
    };
    if let Some(dir) = self.registry_package_dirs.borrow().get(&key) {
      return Ok(RegistryPackageDir {
        dir: dir.clone(),
        is_first_download: false,
      });
    }
    let registry = self.npmrc_chain(base_dir, environment).registry_for_package(name, environment);
    let package = ensure_npm_package_extracted(name, version, &registry, environment).await?;
    self.registry_package_dirs.borrow_mut().insert(key, package.dir.clone());
    Ok(RegistryPackageDir {
      dir: package.dir,
      is_first_download: package.is_first_download,
    })
  }

  fn npmrc_chain(&self, start_dir: Option<&Path>, environment: &impl Environment) -> Rc<NpmrcChain> {
    self
      .npmrc_chains
      .borrow_mut()
      .entry(start_dir.map(|d| d.to_path_buf()))
      .or_insert_with(|| Rc::new(NpmrcChain::load(start_dir, environment)))
      .clone()
  }

  fn read_package_json(&self, package_dir: &Path, environment: &impl Environment) -> Result<Rc<Option<serde_json::Value>>> {
    if let Some(package_json) = self.package_jsons.borrow().get(package_dir) {
      return Ok(package_json.clone());
    }
    let package_json = Rc::new(read_package_json(package_dir, environment)?);
    self.package_jsons.borrow_mut().insert(package_dir.to_path_buf(), package_json.clone());
    Ok(package_json)
  }

  /// Gets the path within the package of the configuration file being referenced.
  ///
  /// The `exports` of the package's package.json have the highest precedence. When
  /// they don't have a match, a sub path is the path of a file in the package and
  /// no sub path is the package.json's `main` if it's a JSON file or otherwise a
  /// dprint.json file at the root of the package.
  ///
  /// Falling back when there's no match is more lenient than Node.js, which only
  /// allows what's in the `exports`. It's done because a package's `exports` are
  /// often only written for its JS and don't have its configuration files.
  fn resolve_config_path_in_package(&self, package_dir: &Path, package_name: &str, sub_path: Option<&str>, environment: &impl Environment) -> Result<String> {
    let package_json = self.read_package_json(package_dir, environment)?;
    let export_key = match sub_path {
      Some(sub_path) => format!("./{}", sub_path),
      None => ".".to_string(),
    };
    let export = match package_json.as_ref().as_ref().and_then(|p| p.get("exports")) {
      Some(exports) => resolve_export(exports, &export_key),
      None => ExportResolution::NotFound,
    };
    match export {
      ExportResolution::Target(target) => {
        let Some(path) = normalize_package_json_path(&target) else {
          bail!(
            "The \"exports\" of npm package {} has an invalid target for \"{}\": {}",
            package_name,
            export_key,
            target
          );
        };
        return Ok(path);
      }
      ExportResolution::Excluded => {
        bail!("The \"exports\" of npm package {} excludes \"{}\".", package_name, export_key);
      }
      ExportResolution::NotFound => {}
    }

    if let Some(sub_path) = sub_path {
      return Ok(sub_path.to_string());
    }

    let main = package_json
      .as_ref()
      .as_ref()
      .and_then(|p| p.get("main"))
      .and_then(|m| m.as_str())
      .filter(|m| is_json_file_name(m))
      .and_then(normalize_package_json_path)
      .filter(|m| environment.path_exists(package_dir.join(m)));
    if let Some(main) = main {
      return Ok(main);
    }

    for file_name in DEFAULT_CONFIG_FILE_NAMES {
      if environment.path_exists(package_dir.join(file_name)) {
        return Ok(file_name.to_string());
      }
    }

    bail!(
      concat!(
        "Could not determine the configuration file of npm package {}. Specify a file in the package ",
        "(ex. npm:{}/config.json) or have the package provide a dprint.json file, a \".\" entry ",
        "in its package.json's \"exports\", or a JSON file as its package.json's \"main\"."
      ),
      package_name,
      package_name,
    );
  }
}

struct RegistryPackageDir {
  dir: PathBuf,
  /// Whether the package was downloaded from the registry instead of being
  /// found in the cache.
  is_first_download: bool,
}

fn read_package_json(package_dir: &Path, environment: &impl Environment) -> Result<Option<serde_json::Value>> {
  let path = package_dir.join("package.json");
  if !environment.path_exists(&path) {
    return Ok(None);
  }
  let text = environment.read_file(&path)?;
  let value = serde_json::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))?;
  Ok(Some(value))
}

#[derive(Debug, PartialEq, Eq)]
enum ExportResolution {
  /// Path of the file being exported (ex. `./src/index.json`).
  Target(String),
  /// The package explicitly doesn't export this (a `null` target).
  Excluded,
  NotFound,
}

/// Resolves a key (ex. `.` or `./sub/path`) in a package.json's `exports`
/// to its target following Node.js' resolution: an exact key has the highest
/// precedence, then the most specific subpath pattern (ex. `./configs/*`).
fn resolve_export(exports: &serde_json::Value, key: &str) -> ExportResolution {
  let sub_paths = match exports {
    serde_json::Value::Object(obj) if obj.keys().any(|k| k.starts_with('.')) => obj,
    // anything else is shorthand for the "." export
    _ if key == "." => return resolve_export_target(exports, None, false),
    _ => return ExportResolution::NotFound,
  };
  if !key.contains('*')
    && let Some(target) = sub_paths.get(key)
  {
    return resolve_export_target(target, None, false);
  }

  // the pattern with the longest text before the `*` wins, then the longest pattern
  let best_match = sub_paths
    .iter()
    .filter_map(|(pattern, target)| {
      let (prefix, suffix) = pattern.split_once('*')?;
      if suffix.contains('*') {
        return None;
      }
      let pattern_match = key.strip_prefix(prefix)?.strip_suffix(suffix)?;
      (!pattern_match.is_empty()).then_some((prefix.len(), pattern.len(), pattern_match, target))
    })
    .max_by_key(|(prefix_len, pattern_len, _, _)| (*prefix_len, *pattern_len));
  match best_match {
    Some((_, _, pattern_match, target)) => resolve_export_target(target, Some(pattern_match), false),
    None => ExportResolution::NotFound,
  }
}

/// Resolves the target of an export. A target that's not within a `dprint`
/// condition is only used when it's a JSON file since the export is otherwise
/// most likely for a JS runtime (ex. `"default": "./index.js"`).
fn resolve_export_target(target: &serde_json::Value, pattern_match: Option<&str>, is_dprint_condition: bool) -> ExportResolution {
  match target {
    serde_json::Value::String(text) => {
      if is_dprint_condition || is_json_file_name(text) {
        ExportResolution::Target(match pattern_match {
          Some(pattern_match) => text.replace('*', pattern_match),
          None => text.to_string(),
        })
      } else {
        ExportResolution::NotFound
      }
    }
    serde_json::Value::Null => ExportResolution::Excluded,
    // the first matching condition that resolves wins
    serde_json::Value::Object(conditions) => conditions
      .iter()
      .filter_map(|(condition, target)| match condition.as_str() {
        "dprint" => Some(resolve_export_target(target, pattern_match, true)),
        "default" => Some(resolve_export_target(target, pattern_match, is_dprint_condition)),
        _ => None,
      })
      .find(|resolution| *resolution != ExportResolution::NotFound)
      .unwrap_or(ExportResolution::NotFound),
    // the first target that can be used wins
    serde_json::Value::Array(targets) => {
      let mut result = ExportResolution::NotFound;
      for target in targets {
        match resolve_export_target(target, pattern_match, is_dprint_condition) {
          ExportResolution::Target(target) => return ExportResolution::Target(target),
          ExportResolution::Excluded => result = ExportResolution::Excluded,
          ExportResolution::NotFound => {}
        }
      }
      result
    }
    _ => ExportResolution::NotFound,
  }
}

/// Converts a relative path in a package.json (ex. `./src/index.json`) to a
/// path within the package (ex. `src/index.json`), returning `None` when it's
/// not a path to a file within the package.
fn normalize_package_json_path(path: &str) -> Option<String> {
  join_package_path("", path).ok().filter(|path| !path.is_empty())
}

/// Resolves a relative path against the directory of a file in a package,
/// erroring when the result is outside the package.
fn join_package_path(from_file_path: &str, relative_path: &str) -> Result<String> {
  if relative_path.starts_with('/') || relative_path.contains('\\') || relative_path.contains(':') {
    bail!("Expected a relative path with forward slashes.");
  }
  let mut segments = from_file_path.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>();
  segments.pop(); // file name
  for segment in relative_path.split('/') {
    match segment {
      "" | "." => {}
      ".." => {
        if segments.pop().is_none() {
          bail!("The path is outside the npm package.");
        }
      }
      segment => segments.push(segment),
    }
  }
  Ok(segments.join("/"))
}

fn read_package_file(package_dir: &Path, package_name: &str, path: &str, environment: &impl Environment) -> Result<Vec<u8>> {
  let file_path = get_existing_package_file_path(package_dir, package_name, path, environment)?;
  Ok(environment.read_file_bytes(&file_path)?)
}

fn get_existing_package_file_path(package_dir: &Path, package_name: &str, path: &str, environment: &impl Environment) -> Result<PathBuf> {
  let file_path = package_dir.join(path);
  if !environment.path_is_file(&file_path) {
    bail!("Could not find {} in npm package {}.", path, package_name);
  }
  Ok(file_path)
}

fn is_json_file_name(file_name: &str) -> bool {
  let file_name = file_name.to_ascii_lowercase();
  file_name.ends_with(".json") || file_name.ends_with(".jsonc")
}

#[cfg(test)]
mod tests {
  use pretty_assertions::assert_eq;

  use crate::arg_parser::parse_args;
  use crate::configuration::ConfigMapValue;
  use crate::configuration::ResolvedConfig;
  use crate::configuration::resolve_config_from_args;
  use crate::environment::CanonicalizedPathBuf;
  use crate::environment::TestEnvironment;
  use crate::environment::TestEnvironmentBuilder;
  use crate::plugins::PluginSourceReference;
  use crate::test_helpers::create_test_npm_tarball;
  use crate::utils::TestStdInReader;
  use dprint_core::configuration::ConfigKeyValue;

  use super::*;

  #[test]
  fn should_extend_main_config_file_in_node_modules() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/project/dprint.json", r#"{ "extends": "npm:@scope/config", "prop1": 1 }"#)
      .write_file(
        "/project/node_modules/@scope/config/dprint.json",
        r#"{
          "plugins": ["./test-plugin.json@checksum"],
          "prop1": 2,
          "prop2": 3
        }"#,
      )
      .build();

    environment.clone().run_in_runtime(async move {
      let result = resolve_config("/project/dprint.json", &environment).await.unwrap();
      // it's a local file, so process plugins are kept and resolved relative to it
      assert_eq!(
        result.plugins,
        vec![PluginSourceReference {
          path_source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/project/node_modules/@scope/config/test-plugin.json")),
          checksum: Some("checksum".to_string()),
        }]
      );
      assert_eq!(get_number(&result, "prop1"), 1);
      assert_eq!(get_number(&result, "prop2"), 3);
      assert!(environment.take_stderr_messages().is_empty());
    });
  }

  #[test]
  fn should_extend_export_and_file_in_ancestor_node_modules() {
    let environment = TestEnvironmentBuilder::new()
      .write_file(
        "/project/sub/dprint.json",
        r#"{ "extends": ["npm:config", "npm:config/malva", "npm:config/other/file.json"] }"#,
      )
      .write_file(
        "/project/node_modules/config/package.json",
        r#"{
          "exports": {
            ".": { "import": "./index.js", "dprint": "./src/index.json" },
            "./malva": "./src/malva.json"
          }
        }"#,
      )
      .write_file("/project/node_modules/config/src/index.json", r#"{ "prop1": 1, "extends": "./base.json" }"#)
      .write_file("/project/node_modules/config/src/base.json", r#"{ "prop2": 2 }"#)
      .write_file("/project/node_modules/config/src/malva.json", r#"{ "prop3": 3 }"#)
      .write_file("/project/node_modules/config/other/file.json", r#"{ "prop4": 4 }"#)
      .build();

    environment.clone().run_in_runtime(async move {
      let result = resolve_config("/project/sub/dprint.json", &environment).await.unwrap();
      assert_eq!(get_number(&result, "prop1"), 1);
      assert_eq!(get_number(&result, "prop2"), 2);
      assert_eq!(get_number(&result, "prop3"), 3);
      assert_eq!(get_number(&result, "prop4"), 4);
    });
  }

  #[test]
  fn should_follow_export_patterns_and_exclusions() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/dprint.json", r#"{ "extends": "npm:config/markdown" }"#)
      .write_file("/excluded.json", r#"{ "extends": "npm:config/internal/base.json" }"#)
      .write_file(
        "/node_modules/config/package.json",
        r#"{ "exports": { "./*": "./configs/*.json", "./internal/*": null } }"#,
      )
      .write_file("/node_modules/config/configs/markdown.json", r#"{ "prop1": 1 }"#)
      .write_file("/node_modules/config/internal/base.json", r#"{ "prop2": 2 }"#)
      .build();

    environment.clone().run_in_runtime(async move {
      let result = resolve_config("/dprint.json", &environment).await.unwrap();
      assert_eq!(get_number(&result, "prop1"), 1);
      assert_eq!(
        resolve_config("/excluded.json", &environment).await.unwrap_err().to_string(),
        "The \"exports\" of npm package config excludes \"./internal/base.json\".",
      );
    });
  }

  #[test]
  fn should_extend_package_json_main_in_node_modules() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/dprint.json", r#"{ "extends": "npm:config" }"#)
      .write_file("/node_modules/config/package.json", r#"{ "main": "index.json" }"#)
      .write_file("/node_modules/config/index.json", r#"{ "prop1": 1 }"#)
      .build();

    environment.clone().run_in_runtime(async move {
      let result = resolve_config("/dprint.json", &environment).await.unwrap();
      assert_eq!(get_number(&result, "prop1"), 1);
    });
  }

  #[test]
  fn should_error_for_node_modules_problems() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/missing.json", r#"{ "extends": "npm:config" }"#)
      .write_file("/no-entry.json", r#"{ "extends": "npm:other" }"#)
      .write_file("/no-file.json", r#"{ "extends": "npm:other/file.json" }"#)
      .write_file("/checksum.json", r#"{ "extends": "npm:other/file.json@checksum" }"#)
      .write_file("/node_modules/other/package.json", r#"{ "main": "index.js" }"#)
      .build();

    environment.clone().run_in_runtime(async move {
      assert_eq!(
        resolve_config("/missing.json", &environment).await.unwrap_err().to_string(),
        concat!(
          "Could not find config in node_modules. Make sure the package is installed (ex. npm install config) ",
          "or specify a version to resolve it from the npm registry (ex. npm:config@x.x.x)."
        ),
      );
      assert_eq!(
        resolve_config("/no-entry.json", &environment).await.unwrap_err().to_string(),
        concat!(
          "Could not determine the configuration file of npm package other. Specify a file in the package ",
          "(ex. npm:other/config.json) or have the package provide a dprint.json file, a \".\" entry ",
          "in its package.json's \"exports\", or a JSON file as its package.json's \"main\"."
        ),
      );
      assert_eq!(
        resolve_config("/no-file.json", &environment).await.unwrap_err().to_string(),
        "Could not find file.json in npm package other.",
      );
      assert_eq!(
        resolve_config("/checksum.json", &environment).await.unwrap_err().to_string(),
        "Checksums are not supported for npm specifiers in \"extends\": npm:other/file.json@checksum",
      );
    });
  }

  #[test]
  fn should_extend_from_registry_as_remote_config() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/dprint.json", r#"{ "extends": "npm:config@1.0.0", "prop1": 1 }"#)
      .build();
    add_registry_package(
      &environment,
      "config",
      "1.0.0",
      &[
        ("package/package.json", r#"{ "exports": "./configs/index.json" }"#),
        (
          "package/configs/index.json",
          r#"{
            "extends": ["./nested/other.json", "npm:second@2.0.0/file.json"],
            "includes": ["**/*.ts"],
            "plugins": ["https://plugins.dprint.dev/test-plugin.wasm", "https://plugins.dprint.dev/test-plugin.json@checksum"],
            "prop1": 2,
            "prop2": 2
          }"#,
        ),
        ("package/configs/nested/other.json", r#"{ "extends": "../../root.json", "prop3": 3 }"#),
        ("package/root.json", r#"{ "prop4": 4 }"#),
      ],
    );
    add_registry_package(&environment, "second", "2.0.0", &[("package/file.json", r#"{ "prop5": 5 }"#)]);

    environment.clone().run_in_runtime(async move {
      let result = resolve_config("/dprint.json", &environment).await.unwrap();
      // remote configuration, so the includes and non-wasm plugins are ignored
      assert_eq!(result.includes, None);
      assert!(!result.config_map.contains_key("includes"));
      assert_eq!(
        result.plugins,
        vec![PluginSourceReference::new_remote_from_str("https://plugins.dprint.dev/test-plugin.wasm")]
      );
      assert_eq!(get_number(&result, "prop1"), 1);
      assert_eq!(get_number(&result, "prop2"), 2);
      assert_eq!(get_number(&result, "prop3"), 3);
      assert_eq!(get_number(&result, "prop4"), 4);
      assert_eq!(get_number(&result, "prop5"), 5);
      assert_eq!(environment.take_stderr_messages().len(), 2);

      // uses the cache the second time
      for url in [
        "https://registry.npmjs.org/config",
        "https://registry.npmjs.org/config/-/config-1.0.0.tgz",
        "https://registry.npmjs.org/second",
        "https://registry.npmjs.org/second/-/second-2.0.0.tgz",
      ] {
        environment.add_remote_file_error(url, "must not be fetched again");
      }
      let second_result = resolve_config("/dprint.json", &environment).await.unwrap();
      assert_eq!(second_result.config_map, result.config_map);
      // only warns about the includes on the first download
      assert_eq!(environment.take_stderr_messages().len(), 1);
    });
  }

  #[test]
  fn should_error_for_registry_problems() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/escape.json", r#"{ "extends": "npm:config@1.0.0/escape.json" }"#)
      .write_file("/node-modules.json", r#"{ "extends": "npm:config@1.0.0/node-modules.json" }"#)
      .write_file("/missing-version.json", r#"{ "extends": "npm:config@2.0.0" }"#)
      .write_file("/home-dir.json", r#"{ "extends": "npm:config@1.0.0/home-dir.json" }"#)
      .write_file("/file-url.json", r#"{ "extends": "npm:config@1.0.0/file-url.json" }"#)
      .write_file("/drive-letter.json", r#"{ "extends": "npm:config@1.0.0/C:/local.json" }"#)
      .write_file("/local.json", r#"{ "plugins": ["./test-plugin.json@checksum"] }"#)
      .build();
    add_registry_package(
      &environment,
      "config",
      "1.0.0",
      &[
        ("package/home-dir.json", r#"{ "extends": "~/local.json" }"#),
        ("package/file-url.json", r#"{ "extends": "file:///local.json" }"#),
        ("package/escape.json", r#"{ "extends": "../other.json" }"#),
        ("package/node-modules.json", r#"{ "extends": "npm:other" }"#),
      ],
    );

    environment.clone().run_in_runtime(async move {
      assert_eq!(
        resolve_config("/escape.json", &environment).await.unwrap_err().to_string(),
        concat!(
          "Failed resolving '../other.json' in the \"extends\" of npm:config@1.0.0/escape.json: The path is outside the npm package.\n",
          "    at npm:config@1.0.0/escape.json"
        ),
      );
      assert_eq!(
        resolve_config("/node-modules.json", &environment).await.unwrap_err().to_string(),
        concat!(
          "Cannot resolve npm:other from node_modules because the configuration file extending it is not a local file ",
          "(npm:config@1.0.0/node-modules.json). Specify a version to resolve it from the npm registry (ex. npm:other@x.x.x).\n",
          "    at npm:config@1.0.0/node-modules.json"
        ),
      );
      assert_eq!(
        resolve_config("/missing-version.json", &environment).await.unwrap_err().to_string(),
        "Version 2.0.0 not found for package config",
      );
      // must never be able to extend a local file, since it would be considered local configuration
      assert_eq!(
        resolve_config("/home-dir.json", &environment).await.unwrap_err().to_string(),
        concat!(
          "Cannot extend '~/local.json' in a configuration file from the npm registry. ",
          "Only relative paths to files in the package, npm specifiers, and http(s) urls are supported.\n",
          "    at npm:config@1.0.0/home-dir.json"
        ),
      );
      assert_eq!(
        resolve_config("/file-url.json", &environment).await.unwrap_err().to_string(),
        concat!(
          "Cannot extend 'file:///local.json' in a configuration file from the npm registry. ",
          "Only relative paths to files in the package, npm specifiers, and http(s) urls are supported.\n",
          "    at npm:config@1.0.0/file-url.json"
        ),
      );
      assert_eq!(
        resolve_config("/drive-letter.json", &environment).await.unwrap_err().to_string(),
        "Plugin path in npm specifier must not contain colons (got 'C:/local.json'): npm:config@1.0.0/C:/local.json",
      );
    });
  }

  #[test]
  fn should_resolve_npm_plugins_in_registry_config_from_extending_config_dir() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/project/dprint.json", r#"{ "extends": "npm:config@1.0.0" }"#)
      .build();
    add_registry_package(
      &environment,
      "config",
      "1.0.0",
      &[("package/dprint.json", r#"{ "plugins": ["npm:@dprint/typescript"] }"#)],
    );

    environment.clone().run_in_runtime(async move {
      let result = resolve_config("/project/dprint.json", &environment).await.unwrap();
      assert_eq!(
        result.plugins,
        vec![PluginSourceReference {
          path_source: PathSource::new_npm(
            NpmSpecifier {
              name: "@dprint/typescript".to_string(),
              version: None,
              path: "plugin.wasm".to_string(),
            },
            Some(CanonicalizedPathBuf::new_for_testing("/project")),
          ),
          checksum: None,
        }]
      );
    });
  }

  #[test]
  fn should_share_extracted_package_with_npm_plugins() {
    use crate::plugins::PluginCache;
    use crate::test_helpers::WASM_PLUGIN_BYTES;

    let environment = TestEnvironmentBuilder::new()
      .write_file("/dprint.json", r#"{ "extends": "npm:config@1.0.0" }"#)
      .build();
    let packument_url = "https://registry.npmjs.org/config";
    let tarball_url = "https://registry.npmjs.org/config/-/config-1.0.0.tgz";
    let files: [(&str, &[u8]); 3] = [
      ("package/dprint.json", br#"{ "prop1": 1 }"#),
      ("package/plugin.wasm", WASM_PLUGIN_BYTES),
      ("package/plugin.json", b"{}"),
    ];
    let tarball = create_test_npm_tarball(&files);
    let checksum = crate::utils::get_sha256_checksum(&tarball);
    let packument = serde_json::json!({ "versions": { "1.0.0": { "dist": { "tarball": tarball_url } } } });
    environment.add_remote_file_bytes(packument_url, packument.to_string().into_bytes());
    environment.add_remote_file_bytes(tarball_url, tarball);

    environment.clone().run_in_runtime(async move {
      let result = resolve_config("/dprint.json", &environment).await.unwrap();
      assert_eq!(get_number(&result, "prop1"), 1);

      // the plugin uses the package the config was extended from
      environment.add_remote_file_error(packument_url, "must not be fetched again");
      environment.add_remote_file_error(tarball_url, "must not be fetched again");
      let plugin_cache = PluginCache::new(environment.clone());
      let plugin = |text: &str| crate::plugins::parse_plugin_source_reference(text, &PathSource::new_local(environment.cwd()), &environment).unwrap();
      let cache_item = plugin_cache
        .get_plugin_cache_item(&plugin(&format!("npm:config@1.0.0@{}", checksum)))
        .await
        .unwrap();
      assert_eq!(cache_item.info.name, "test-plugin");
      let _ = environment.take_stderr_messages(); // wasm compile message

      // and still verifies the checksum of the package's tarball
      let err = plugin_cache.get_plugin_cache_item(&plugin("npm:config@1.0.0/plugin.json")).await.err().unwrap();
      assert!(err.to_string().contains("must have a checksum specified"), "{:#}", err);
      let err = plugin_cache
        .get_plugin_cache_item(&plugin("npm:config@1.0.0/plugin.json@wrong"))
        .await
        .err()
        .unwrap();
      assert_eq!(
        err.to_string(),
        format!(
          "Invalid checksum for npm package npm:config@1.0.0/plugin.json. Check the plugin's release notes for the expected checksum.

Actual: {}
Expected: wrong",
          checksum
        ),
      );
      // what couldn't be verified isn't kept
      assert!(!environment.path_exists(environment.get_cache_dir().join("npm/registry.npmjs.org/config@1.0.0")));
    });
  }

  #[test]
  fn should_download_package_once_when_used_at_same_time() {
    use crate::plugins::PluginCache;
    use crate::test_helpers::WASM_PLUGIN_BYTES;

    let environment = TestEnvironmentBuilder::new()
      .write_file(
        "/dprint.json",
        r#"{ "extends": ["npm:config@1.0.0/a.json", "npm:config@1.0.0/b.json", "npm:config@1.0.0/c.json"] }"#,
      )
      .build();
    let packument_url = "https://registry.npmjs.org/config";
    let tarball_url = "https://registry.npmjs.org/config/-/config-1.0.0.tgz";
    let files: [(&str, &[u8]); 5] = [
      ("package/a.json", br#"{ "prop1": 1 }"#),
      ("package/b.json", br#"{ "prop2": 2 }"#),
      ("package/c.json", br#"{ "prop3": 3 }"#),
      ("package/one/plugin.wasm", WASM_PLUGIN_BYTES),
      ("package/two/plugin.wasm", WASM_PLUGIN_BYTES),
    ];
    let packument = serde_json::json!({ "versions": { "1.0.0": { "dist": { "tarball": tarball_url } } } });
    environment.add_remote_file_bytes(packument_url, packument.to_string().into_bytes());
    environment.add_remote_file_bytes(tarball_url, create_test_npm_tarball(&files));

    environment.clone().run_in_runtime(async move {
      // extends of several files in a package
      let result = resolve_config("/dprint.json", &environment).await.unwrap();
      assert_eq!(get_number(&result, "prop1"), 1);
      assert_eq!(get_number(&result, "prop2"), 2);
      assert_eq!(get_number(&result, "prop3"), 3);
      assert_eq!(environment.remote_file_request_count(packument_url), 1);
      assert_eq!(environment.remote_file_request_count(tarball_url), 1);

      // plugins at different sub paths of a package
      environment.remove_dir_all(environment.get_cache_dir().join("npm")).unwrap();
      let plugin_cache = PluginCache::new(environment.clone());
      let plugin = |text: &str| crate::plugins::parse_plugin_source_reference(text, &PathSource::new_local(environment.cwd()), &environment).unwrap();
      let (one, two) = (plugin("npm:config@1.0.0/one/plugin.wasm"), plugin("npm:config@1.0.0/two/plugin.wasm"));
      let (one, two) = dprint_core::async_runtime::future::join(plugin_cache.get_plugin_cache_item(&one), plugin_cache.get_plugin_cache_item(&two)).await;
      assert_eq!(one.unwrap().info.name, "test-plugin");
      assert_eq!(two.unwrap().info.name, "test-plugin");
      assert_eq!(environment.remote_file_request_count(packument_url), 2);
      assert_eq!(environment.remote_file_request_count(tarball_url), 2);
      let _ = environment.take_stderr_messages(); // wasm compile messages
    });
  }

  #[test]
  fn should_only_read_npmrc_and_package_json_once() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/project/.npmrc", "registry=https://npm.example.com/")
      .build();
    let packument_url = "https://npm.example.com/config";
    let tarball_url = "https://npm.example.com/config/-/config-1.0.0.tgz";
    let files: [(&str, &[u8]); 4] = [
      ("package/package.json", br#"{ "exports": { "./a": "./a.json", "./b": "./b.json" } }"#),
      ("package/a.json", br#"{ "prop1": 1 }"#),
      ("package/b.json", br#"{ "prop2": 2 }"#),
      ("package/c.json", br#"{ "prop3": 3 }"#),
    ];
    let packument = serde_json::json!({ "versions": { "1.0.0": { "dist": { "tarball": tarball_url } } } });
    environment.add_remote_file_bytes(packument_url, packument.to_string().into_bytes());
    environment.add_remote_file_bytes(tarball_url, create_test_npm_tarball(&files));

    environment.clone().run_in_runtime(async move {
      let resolver = NpmExtendsResolver::default();
      let base = PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/project"));
      let file = resolver.resolve("npm:config@1.0.0/a", &base, &environment).await.unwrap();
      assert_eq!(file.source.display(), "npm:config@1.0.0/a.json");
      assert!(file.is_first_download);

      // changing these has no effect because what was read is reused
      let package_dir = environment.get_cache_dir().join("npm/npm.example.com/config@1.0.0");
      environment.write_file(package_dir.join("package.json"), "{}").unwrap();
      environment.remove_file("/project/.npmrc").unwrap();

      let file = resolver.resolve("npm:config@1.0.0/b", &base, &environment).await.unwrap();
      assert_eq!(file.source.display(), "npm:config@1.0.0/b.json");
      assert!(!file.is_first_download);
      let PathSource::Npm(npm_source) = &file.source else { unreachable!() };
      let file = resolver.resolve_relative("./c.json", npm_source, &environment).await.unwrap();
      assert_eq!(file.source.display(), "npm:config@1.0.0/c.json");
      assert_eq!(environment.remote_file_request_count(tarball_url), 1);
    });
  }

  #[test]
  fn should_consider_local_file_extended_by_remote_config_as_remote() {
    let local_config = r#"{
      "includes": ["**/*.ts"],
      "plugins": ["./test-plugin.json@checksum", "./test-plugin.wasm"],
      "prop1": 1
    }"#;
    let environment = TestEnvironmentBuilder::new()
      .write_file("/url.json", r#"{ "extends": "https://dprint.dev/config.json" }"#)
      .write_file("/npm.json", r#"{ "extends": "npm:config@1.0.0" }"#)
      .write_file("/local.json", r#"{ "extends": "./dir/local.json" }"#)
      .write_file("/home/local.json", local_config)
      .write_file("/dir/local.json", local_config)
      .build();
    environment.add_remote_file_bytes("https://dprint.dev/config.json", br#"{ "extends": "~/local.json" }"#.to_vec());
    add_registry_package(
      &environment,
      "config",
      "1.0.0",
      &[("package/dprint.json", r#"{ "extends": "https://dprint.dev/config.json" }"#)],
    );

    environment.clone().run_in_runtime(async move {
      for config_path in ["/url.json", "/npm.json"] {
        let result = resolve_config(config_path, &environment).await.unwrap();
        assert_eq!(get_number(&result, "prop1"), 1);
        // the includes and non-wasm plugins are ignored
        assert_eq!(result.includes, None);
        assert!(!result.config_map.contains_key("includes"));
        assert_eq!(
          result.plugins,
          vec![PluginSourceReference {
            path_source: PathSource::new_local(CanonicalizedPathBuf::new_for_testing("/home/test-plugin.wasm")),
            checksum: None,
          }]
        );
        assert!(!environment.take_stderr_messages().is_empty());
      }

      // not when only extended by local config
      let result = resolve_config("/local.json", &environment).await.unwrap();
      assert_eq!(result.plugins.len(), 2);
      assert!(environment.take_stderr_messages().is_empty());
    });
  }

  #[test]
  fn should_error_for_circular_extends() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/self.json", r#"{ "extends": "./self.json" }"#)
      .write_file("/a.json", r#"{ "extends": "./dir/b.json" }"#)
      .write_file("/dir/b.json", r#"{ "extends": "npm:config@1.0.0" }"#)
      .write_file("/diamond.json", r#"{ "extends": ["./left.json", "./right.json"] }"#)
      .write_file("/left.json", r#"{ "extends": "./shared.json", "prop1": 1 }"#)
      .write_file("/right.json", r#"{ "extends": "./shared.json", "prop2": 2 }"#)
      .write_file("/shared.json", r#"{ "prop3": 3 }"#)
      .build();
    add_registry_package(
      &environment,
      "config",
      "1.0.0",
      &[
        ("package/dprint.json", r#"{ "extends": "./other.json" }"#),
        ("package/other.json", r#"{ "extends": "npm:config@1.0.0/dprint.json" }"#),
      ],
    );

    environment.clone().run_in_runtime(async move {
      assert_eq!(
        resolve_config("/self.json", &environment).await.unwrap_err().to_string(),
        "Circular extends detected: /self.json -> /self.json",
      );
      assert_eq!(
        resolve_config("/a.json", &environment).await.unwrap_err().to_string(),
        concat!(
          "Circular extends detected: /a.json -> /dir/b.json -> npm:config@1.0.0/dprint.json -> npm:config@1.0.0/other.json -> npm:config@1.0.0/dprint.json
",
          "    at npm:config@1.0.0/other.json
",
          "    at npm:config@1.0.0/dprint.json
",
          "    at /dir/b.json"
        ),
      );

      // extending the same file in several places is fine
      let result = resolve_config("/diamond.json", &environment).await.unwrap();
      assert_eq!(get_number(&result, "prop1"), 1);
      assert_eq!(get_number(&result, "prop2"), 2);
      assert_eq!(get_number(&result, "prop3"), 3);
    });
  }

  #[test]
  fn should_resolve_exports() {
    fn target(text: &str) -> ExportResolution {
      ExportResolution::Target(text.to_string())
    }

    let text = serde_json::json!("./index.json");
    assert_eq!(resolve_export(&text, "."), target("./index.json"));
    assert_eq!(resolve_export(&text, "./sub"), ExportResolution::NotFound);

    let conditions = serde_json::json!({ "import": "./index.js", "default": "./index.json" });
    assert_eq!(resolve_export(&conditions, "."), target("./index.json"));
    assert_eq!(resolve_export(&conditions, "./sub"), ExportResolution::NotFound);

    let sub_paths = serde_json::json!({
      ".": [{ "import": "./index.js" }, "./index.json"],
      "./sub": { "dprint": { "default": "./sub.json" }, "default": "./sub.js" },
      "./none": null,
      "./condition-none": { "dprint": null, "default": "./other.json" },
    });
    assert_eq!(resolve_export(&sub_paths, "."), target("./index.json"));
    assert_eq!(resolve_export(&sub_paths, "./sub"), target("./sub.json"));
    assert_eq!(resolve_export(&sub_paths, "./none"), ExportResolution::Excluded);
    assert_eq!(resolve_export(&sub_paths, "./condition-none"), ExportResolution::Excluded);
    assert_eq!(resolve_export(&sub_paths, "./other"), ExportResolution::NotFound);

    // only uses a non-JSON file when it's for the dprint condition
    assert_eq!(resolve_export(&serde_json::json!("./index.js"), "."), ExportResolution::NotFound);
    assert_eq!(
      resolve_export(&serde_json::json!({ "import": "./index.mjs", "default": "./index.js" }), "."),
      ExportResolution::NotFound
    );
    assert_eq!(resolve_export(&serde_json::json!({ "dprint": "./config" }), "."), target("./config"));

    // subpath patterns
    let patterns = serde_json::json!({
      "./*": "./configs/*.json",
      "./langs/*": "./configs/langs/*/config.json",
      "./langs/*.json": "./configs/langs/*.json",
      "./langs/internal/*": null,
      "./langs/exact": "./exact.json",
    });
    assert_eq!(resolve_export(&patterns, "./markdown"), target("./configs/markdown.json"));
    assert_eq!(resolve_export(&patterns, "./a/b"), target("./configs/a/b.json"));
    assert_eq!(resolve_export(&patterns, "./langs/ts"), target("./configs/langs/ts/config.json"));
    assert_eq!(resolve_export(&patterns, "./langs/ts.json"), target("./configs/langs/ts.json"));
    assert_eq!(resolve_export(&patterns, "./langs/internal/ts"), ExportResolution::Excluded);
    assert_eq!(resolve_export(&patterns, "./langs/exact"), target("./exact.json"));
    assert_eq!(resolve_export(&patterns, "."), ExportResolution::NotFound);
  }

  #[test]
  fn should_join_package_paths() {
    assert_eq!(join_package_path("index.json", "./other.json").unwrap(), "other.json");
    assert_eq!(join_package_path("a/b/index.json", "../c/other.json").unwrap(), "a/c/other.json");
    assert_eq!(join_package_path("a/index.json", "b//./other.json").unwrap(), "a/b/other.json");
    assert!(join_package_path("a/index.json", "../../other.json").is_err());
    assert!(join_package_path("a/index.json", "/other.json").is_err());
    assert!(join_package_path("a/index.json", "..\\other.json").is_err());
    assert!(join_package_path("a/index.json", "C:/other.json").is_err());
  }

  async fn resolve_config(config_path: &str, environment: &TestEnvironment) -> Result<ResolvedConfig> {
    let args = parse_args(
      vec![String::new(), "check".to_string(), "-c".to_string(), config_path.to_string()],
      TestStdInReader::default(),
    )
    .unwrap();
    Ok(resolve_config_from_args(&args, environment).await?)
  }

  fn add_registry_package(environment: &TestEnvironment, name: &str, version: &str, files: &[(&str, &str)]) {
    let tarball_url = format!("https://registry.npmjs.org/{name}/-/{name}-{version}.tgz");
    let packument = serde_json::json!({
      "versions": {
        version: {
          "dist": { "tarball": tarball_url }
        }
      }
    });
    let files = files.iter().map(|(path, text)| (*path, text.as_bytes())).collect::<Vec<_>>();
    environment.add_remote_file_bytes(&format!("https://registry.npmjs.org/{name}"), packument.to_string().into_bytes());
    environment.add_remote_file_bytes(&tarball_url, create_test_npm_tarball(&files));
  }

  fn get_number(config: &ResolvedConfig, key: &str) -> i32 {
    match config.config_map.get(key) {
      Some(ConfigMapValue::KeyValue(ConfigKeyValue::Number(value))) => *value,
      value => panic!("Expected a number for {}, but found {:?}", key, value),
    }
  }
}

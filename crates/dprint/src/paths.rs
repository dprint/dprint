use anyhow::Context;
use anyhow::Result;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::str::Split;
use thiserror::Error;

use crate::arg_parser::ConfigDiscovery;
use crate::arg_parser::FilePatternArgs;
use crate::configuration::ResolvedConfig;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::patterns::get_all_file_patterns;
use crate::patterns::process_cli_path_args;
use crate::patterns::process_config_patterns;
use crate::plugins::PluginNameResolutionMaps;
use crate::resolution::PluginWithConfig;
use crate::utils::GlobOptions;
use crate::utils::GlobOutput;
use crate::utils::GlobPattern;
use crate::utils::GlobPatterns;
use crate::utils::glob;
use crate::utils::is_negated_glob;
use crate::utils::read_file_shebang_line;

/// Struct that allows using plugin names as a key
/// in a hash map.
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct PluginNames(String);

impl PluginNames {
  const SEPARATOR: &'static str = "~~";

  pub fn names(&self) -> Split<'_, &str> {
    self.0.split(PluginNames::SEPARATOR)
  }
}

impl Borrow<str> for PluginNames {
  fn borrow(&self) -> &str {
    &self.0
  }
}

/// Builds `PluginNames` keys in a buffer it reuses, so that resolving the
/// plugins for many files doesn't allocate a key for every one of them.
#[derive(Default)]
struct PluginNamesBuilder {
  buffer: String,
}

impl PluginNamesBuilder {
  fn build(&mut self, names: &[&str]) -> PluginNamesKey<'_> {
    self.buffer.clear();
    for (i, name) in names.iter().enumerate() {
      if i > 0 {
        self.buffer.push_str(PluginNames::SEPARATOR);
      }
      self.buffer.push_str(name);
    }
    PluginNamesKey(&self.buffer)
  }
}

/// A key borrowed from a `PluginNamesBuilder`, which can be looked up without
/// allocating and only turned into a `PluginNames` when it's not in the map yet.
struct PluginNamesKey<'a>(&'a str);

impl PluginNamesKey<'_> {
  fn as_str(&self) -> &str {
    self.0
  }

  fn to_plugin_names(&self) -> PluginNames {
    PluginNames(self.0.to_string())
  }
}

#[derive(Debug, Error)]
#[error("No files found to format with the specified plugins at {}. You may want to try using `dprint output-file-paths` to see which files it's finding or run with `--allow-no-files`.", .base_path.display())]
pub struct NoFilesFoundError {
  pub base_path: CanonicalizedPathBuf,
}

pub struct FilesPathsByPlugins(HashMap<PluginNames, Vec<PathBuf>>);

impl FilesPathsByPlugins {
  pub fn ensure_not_empty(&self, base_path: &CanonicalizedPathBuf) -> Result<(), NoFilesFoundError> {
    if self.is_empty() {
      Err(NoFilesFoundError { base_path: base_path.clone() })
    } else {
      Ok(())
    }
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  pub fn into_vec(self) -> Vec<(PluginNames, Vec<PathBuf>)> {
    self.0.into_iter().collect()
  }

  pub fn all_file_paths(&self) -> impl Iterator<Item = &PathBuf> {
    self.0.values().flatten()
  }
}

pub fn get_file_paths_by_plugins(
  plugin_name_maps: &PluginNameResolutionMaps,
  file_paths: Vec<PathBuf>,
  mut shebang_lines: HashMap<PathBuf, Vec<u8>>,
  environment: &impl Environment,
) -> Result<FilesPathsByPlugins> {
  let mut file_paths_by_plugin: HashMap<PluginNames, Vec<PathBuf>> = HashMap::new();
  let mut plugin_names_builder = PluginNamesBuilder::default();

  for file_path in file_paths.into_iter() {
    let plugin_names = match shebang_lines.remove(&file_path) {
      // the traversal already read this file's shebang line, so don't read it again
      Some(shebang_line) => plugin_name_maps.get_plugin_names_from_file_path_and_bytes(&file_path, &shebang_line),
      None => get_plugin_names_for_file_on_disk(plugin_name_maps, &file_path, environment),
    };
    if !plugin_names.is_empty() {
      // only a handful of distinct keys exist no matter how many files there
      // are, so allocate one only when the key hasn't been seen yet
      let key = plugin_names_builder.build(&plugin_names);
      match file_paths_by_plugin.get_mut(key.as_str()) {
        Some(file_paths) => file_paths.push(file_path),
        None => {
          file_paths_by_plugin.insert(key.to_plugin_names(), vec![file_path]);
        }
      }
    }
  }

  Ok(FilesPathsByPlugins(file_paths_by_plugin))
}

/// Resolves the plugins for a file on disk, reading its shebang line when it's
/// an extensionless file that didn't match a plugin by path.
pub fn get_plugin_names_for_file_on_disk<'a>(plugin_name_maps: &'a PluginNameResolutionMaps, file_path: &Path, environment: &impl Environment) -> Vec<&'a str> {
  let plugin_names = plugin_name_maps.get_plugin_names_from_file_path(file_path);
  if !plugin_names.is_empty() || !plugin_name_maps.may_match_shebang(file_path) {
    return plugin_names;
  }
  match read_file_shebang_line(environment, file_path) {
    Ok(Some(shebang_line)) => plugin_name_maps.get_plugin_names_from_shebang(file_path, &shebang_line),
    // ex. the file doesn't exist or has no shebang
    _ => Vec::new(),
  }
}

pub async fn get_and_resolve_file_paths<'a>(
  config: &ResolvedConfig,
  args: &FilePatternArgs,
  config_discovery: ConfigDiscovery,
  plugins: impl Iterator<Item = &'a PluginWithConfig>,
  environment: &impl Environment,
) -> Result<GlobOutput> {
  let cwd = environment.cwd();
  let mut file_patterns = get_all_file_patterns(config, args, &cwd, environment);

  if args.only_staged {
    let staged_files = environment.get_staged_files().context("Failed running git staged.")?;
    file_patterns.arg_includes = Some(process_cli_path_args(&staged_files, &cwd, environment));
  } else if args.only_dirty {
    let dirty_files = environment.get_dirty_files().context("Failed running git status.")?;
    file_patterns.arg_includes = Some(process_cli_path_args(&dirty_files, &cwd, environment));
  }

  if file_patterns.config_includes.is_none() {
    // If no includes patterns were specified, derive one from the list of plugins
    // as this is a massive performance improvement, because it collects less file
    // paths to examine and match to plugins later.
    //
    // These are based at the config dir rather than the cwd so that explicitly
    // specified paths outside the cwd (ex. ../file.txt) can still match them.
    file_patterns.config_includes = Some(GlobPattern::new_vec(get_plugin_patterns(plugins), config.base_path.clone()));
  }

  get_and_resolve_file_patterns(config, file_patterns, args.no_gitignore, config_discovery, environment).await
}

async fn get_and_resolve_file_patterns(
  config: &ResolvedConfig,
  file_patterns: GlobPatterns,
  no_gitignore: bool,
  config_discovery: ConfigDiscovery,
  environment: &impl Environment,
) -> Result<GlobOutput> {
  let cwd = environment.cwd();
  let is_cwd_in_base = cwd.starts_with(&config.base_path);
  let is_in_sub_dir = cwd != config.base_path && is_cwd_in_base;
  let start_dir = if is_in_sub_dir { cwd } else { config.base_path.clone() };
  let environment = environment.clone();
  let pattern_base = config.base_path.clone();
  let current_config_path = config.source.maybe_local_path().map(|p| p.as_ref().to_path_buf());

  // This is intensive so do it in a blocking task
  dprint_core::async_runtime::spawn_blocking(move || {
    glob(
      &environment,
      GlobOptions {
        start_dir: start_dir.into_path_buf(),
        file_patterns,
        pattern_base,
        config_discovery,
        current_config_path,
        no_gitignore,
      },
    )
  })
  .await
  .unwrap()
}

fn get_plugin_patterns<'a>(plugins: impl Iterator<Item = &'a PluginWithConfig>) -> Vec<String> {
  let mut file_names = HashSet::new();
  let mut file_exts = HashSet::new();
  let mut association_globs = Vec::new();
  for plugin in plugins {
    // associations add to the plugin's default file matching, so always include
    // the plugin's default file names and extensions plus any positive globs
    file_names.extend(&plugin.file_matching.file_names);
    file_exts.extend(&plugin.file_matching.file_extensions);
    if let Some(associations) = plugin.associations.as_ref() {
      for pattern in process_config_patterns(associations) {
        if !is_negated_glob(&pattern) {
          association_globs.push(pattern);
        }
      }
    }
  }
  let mut result = Vec::new();
  if !file_exts.is_empty() {
    result.push(format!("**/*.{{{}}}", file_exts.into_iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")));
  }
  if !file_names.is_empty() {
    result.push(format!("**/{{{}}}", file_names.into_iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")));
  }
  // add the association globs last as they're least likely to be matched
  result.extend(association_globs);

  result
}

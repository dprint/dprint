use anyhow::Result;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::Path;

use crate::environment::CanonicalizedPathBuf;
use crate::patterns::get_patterns_as_glob_matcher;
use crate::resolution::PluginWithConfig;
use crate::utils::GlobMatcher;
use crate::utils::GlobMatchesDetail;
use crate::utils::get_lowercase_file_extension;
use crate::utils::get_lowercase_file_name;
use crate::utils::get_shebang_line;
use crate::utils::is_shebang_prefix_match;

#[derive(Default)]
pub struct PluginNameResolutionMaps {
  /// The scope's plugins in configuration order, which is the order they
  /// format a file in.
  plugins: Vec<PluginEntry>,
  /// Whether any plugin is additive, so the usual case of none being additive
  /// doesn't pay for looking at plugins that can't match the file anyway.
  has_additive: bool,
  /// Indexes into `plugins`, kept in configuration order.
  extension_to_plugin_indexes_map: HashMap<String, Vec<usize>>,
  /// Indexes into `plugins`, kept in configuration order.
  file_name_to_plugin_indexes_map: HashMap<String, Vec<usize>>,
  /// Indexes into `plugins` of the non-additive plugins that have
  /// `associations`, so claiming a file doesn't have to look at the others.
  association_indexes: Vec<usize>,
  /// Maps a file's shebang line to a file extension so extensionless scripts
  /// can be routed to a plugin. Sorted by shebang length descending so the
  /// most specific entry matches first.
  shebang_to_extension: Vec<(String, String)>,
}

impl PluginNameResolutionMaps {
  pub fn from_plugins<'a>(
    plugins: impl Iterator<Item = &'a PluginWithConfig>,
    config_base_path: &CanonicalizedPathBuf,
    shebangs: Option<&IndexMap<String, String>>,
  ) -> Result<Self> {
    let mut plugin_name_maps = PluginNameResolutionMaps::default();
    if let Some(shebangs) = shebangs {
      plugin_name_maps.shebang_to_extension = shebangs.iter().map(|(shebang, extension)| (shebang.clone(), extension.clone())).collect();
      // longest first so a more specific shebang wins (ex. `deno run` over `deno`)
      plugin_name_maps
        .shebang_to_extension
        .sort_by_key(|(shebang, _)| std::cmp::Reverse(shebang.len()));
    }
    for plugin in plugins {
      let index = plugin_name_maps.plugins.len();
      let additive = plugin.file_matching.additive;

      for extension in &plugin.file_matching.file_extensions {
        plugin_name_maps
          .extension_to_plugin_indexes_map
          .entry(extension.to_lowercase())
          .or_default()
          .push(index);
      }
      for file_name in &plugin.file_matching.file_names {
        plugin_name_maps
          .file_name_to_plugin_indexes_map
          .entry(file_name.to_lowercase())
          .or_default()
          .push(index);
      }

      let associations = get_plugin_association_glob_matcher(plugin, config_base_path)?;
      // an additive plugin never claims a file, so it's matched on its own
      if associations.is_some() && !additive {
        plugin_name_maps.association_indexes.push(index);
      }
      plugin_name_maps.has_additive |= additive;
      plugin_name_maps.plugins.push(PluginEntry {
        name: plugin.name().to_string(),
        additive,
        associations,
      });
    }
    Ok(plugin_name_maps)
  }

  pub fn get_plugin_names_from_file_path<'a>(&'a self, file_path: &Path) -> PathPluginNames<'a> {
    let file_name = get_lowercase_file_name(file_path);
    let extension = get_lowercase_file_extension(file_path);
    let claiming_plugins = self.claiming_plugins(file_path, file_name.as_deref(), extension.as_deref());
    PathPluginNames {
      has_claiming_plugin: !claiming_plugins.is_empty(),
      names: self.get_plugin_names(&claiming_plugins, file_path, file_name.as_deref(), extension.as_deref()),
    }
  }

  /// Resolves plugins for a file, falling back to its shebang line when it's
  /// extensionless and no plugin claimed it by path. `file_bytes_start` only
  /// needs to contain the start of the file.
  pub fn get_plugin_names_from_file_path_and_bytes<'a>(&'a self, file_path: &Path, file_bytes_start: &[u8]) -> Vec<&'a str> {
    let path_plugin_names = self.get_plugin_names_from_file_path(file_path);
    if path_plugin_names.has_claiming_plugin() {
      return path_plugin_names.into_names();
    }
    // an additive plugin matching by path doesn't claim the file, so the
    // shebang still gets to say which plugin formats it
    self
      .get_plugin_names_from_shebang(file_path, file_bytes_start)
      .unwrap_or_else(|| path_plugin_names.into_names())
  }

  /// Whether the file might be resolved by its shebang line. This is the case
  /// when shebang mappings are configured and the file has no extension.
  pub fn may_match_shebang(&self, file_path: &Path) -> bool {
    !self.shebang_to_extension.is_empty() && file_path.extension().is_none()
  }

  /// Resolves plugins for an extensionless file based on its first line (the
  /// shebang). The shebang is looked up in the configured mapping to get an
  /// extension, then the plugins for that extension are resolved. Association
  /// patterns are evaluated against the real file path. `None` means the file's
  /// shebang didn't resolve to an extension, so the caller keeps whatever the
  /// file's path matched.
  ///
  /// A configured shebang matches when the file's shebang line equals it or
  /// starts with it followed by whitespace, so `#!/usr/bin/env deno run` matches
  /// `#!/usr/bin/env deno run --allow-read` but not `#!/usr/bin/env deno runtest`.
  pub fn get_plugin_names_from_shebang<'a>(&'a self, file_path: &Path, file_bytes_start: &[u8]) -> Option<Vec<&'a str>> {
    if !self.may_match_shebang(file_path) {
      return None;
    }
    let shebang = get_shebang_line(file_bytes_start)?;
    let extension = self
      .shebang_to_extension
      .iter()
      .find(|(configured_shebang, _)| is_shebang_prefix_match(shebang, configured_shebang))
      .map(|(_, extension)| extension)?;
    // the shebang says what the file's contents are, so it decides which plugin
    // claims the file—an additive plugin still matches by the file's own name
    let file_name = get_lowercase_file_name(file_path);
    let claiming_plugins = self.claiming_plugins(file_path, None, Some(extension));
    Some(self.get_plugin_names(&claiming_plugins, file_path, file_name.as_deref(), Some(extension)))
  }

  /// The plugins that format the file, in configuration order: the plugins that
  /// claim it plus every additive plugin that matches it.
  fn get_plugin_names<'a>(&'a self, claiming_plugins: &ClaimingPlugins, file_path: &Path, file_name: Option<&str>, extension: Option<&str>) -> Vec<&'a str> {
    if !self.has_additive {
      return claiming_plugins.indexes().iter().map(|&index| self.plugins[index].name.as_str()).collect();
    }
    self
      .plugins
      .iter()
      .enumerate()
      .filter(|(index, plugin)| {
        if plugin.additive {
          self.matches_file(*index, file_path, file_name, extension)
        } else {
          claiming_plugins.contains(*index)
        }
      })
      .map(|(_, plugin)| plugin.name.as_str())
      .collect()
  }

  /// The non-additive plugins that claim the file: the ones whose associations
  /// match it, otherwise the first that matches it by file name, otherwise the
  /// first that matches it by extension.
  fn claiming_plugins(&self, file_path: &Path, file_name: Option<&str>, extension: Option<&str>) -> ClaimingPlugins {
    let association_indexes = self
      .association_indexes
      .iter()
      .copied()
      .filter(|&index| self.matches_associations(index, file_path))
      .collect::<Vec<_>>();
    if !association_indexes.is_empty() {
      return ClaimingPlugins::Many(association_indexes);
    }

    let index = file_name
      .and_then(|file_name| self.first_claiming_index(&self.file_name_to_plugin_indexes_map, file_name, file_path))
      .or_else(|| extension.and_then(|extension| self.first_claiming_index(&self.extension_to_plugin_indexes_map, extension, file_path)));
    match index {
      Some(index) => ClaimingPlugins::One(index),
      None => ClaimingPlugins::None,
    }
  }

  /// The first non-additive plugin in the map's entry that isn't excluded from
  /// the file by its own associations.
  fn first_claiming_index(&self, map: &HashMap<String, Vec<usize>>, key: &str, file_path: &Path) -> Option<usize> {
    map
      .get(key)?
      .iter()
      .copied()
      .find(|&index| !self.plugins[index].additive && self.is_not_associations_excluded(index, file_path))
  }

  /// Whether a plugin matches a file by its associations, or by its default
  /// file name and extension matching when its associations don't exclude it.
  fn matches_file(&self, index: usize, file_path: &Path, file_name: Option<&str>, extension: Option<&str>) -> bool {
    if self.matches_associations(index, file_path) {
      return true;
    }
    let matches_default = file_name.is_some_and(|file_name| map_contains(&self.file_name_to_plugin_indexes_map, file_name, index))
      || extension.is_some_and(|extension| map_contains(&self.extension_to_plugin_indexes_map, extension, index));
    matches_default && self.is_not_associations_excluded(index, file_path)
  }

  fn matches_associations(&self, index: usize, file_path: &Path) -> bool {
    self.plugins[index].associations.as_ref().is_some_and(|matcher| matcher.matches(file_path))
  }

  fn is_not_associations_excluded(&self, index: usize, file_path: &Path) -> bool {
    // `associations` add to the plugin's default file matching, so a plugin
    // keeps matching by its default extension/file name unless a negated
    // association pattern explicitly excludes the file
    match &self.plugins[index].associations {
      Some(matcher) => matcher.matches_detail(file_path) != GlobMatchesDetail::Excluded,
      None => true,
    }
  }
}

/// The plugins that format a file based on its path.
pub struct PathPluginNames<'a> {
  names: Vec<&'a str>,
  has_claiming_plugin: bool,
}

impl<'a> PathPluginNames<'a> {
  /// Whether a plugin claimed the file. When none did, an extensionless file's
  /// shebang line may still route it to a plugin, so it's worth reading.
  pub fn has_claiming_plugin(&self) -> bool {
    self.has_claiming_plugin
  }

  pub fn into_names(self) -> Vec<&'a str> {
    self.names
  }
}

/// A plugin as it takes part in file matching.
struct PluginEntry {
  name: String,
  /// Whether the plugin formats a file it matches in addition to the plugin
  /// that claims it, rather than claiming the file itself.
  additive: bool,
  /// The plugin's `associations` matcher, which adds to its default file
  /// matching. A negated pattern excludes a file it would match by default.
  associations: Option<GlobMatcher>,
}

/// The plugins claiming a file, as indexes into [`PluginNameResolutionMaps`]'s
/// plugins. Only `associations` can have several plugins claim the same file.
enum ClaimingPlugins {
  None,
  One(usize),
  Many(Vec<usize>),
}

impl ClaimingPlugins {
  pub fn indexes(&self) -> &[usize] {
    match self {
      ClaimingPlugins::None => &[],
      ClaimingPlugins::One(index) => std::slice::from_ref(index),
      ClaimingPlugins::Many(indexes) => indexes,
    }
  }

  pub fn is_empty(&self) -> bool {
    self.indexes().is_empty()
  }

  pub fn contains(&self, index: usize) -> bool {
    self.indexes().contains(&index)
  }
}

fn map_contains(map: &HashMap<String, Vec<usize>>, key: &str, index: usize) -> bool {
  map.get(key).is_some_and(|indexes| indexes.contains(&index))
}

fn get_plugin_association_glob_matcher(plugin: &PluginWithConfig, config_base_path: &CanonicalizedPathBuf) -> Result<Option<GlobMatcher>> {
  match plugin.associations.as_deref() {
    Some(associations) => Ok(Some(get_patterns_as_glob_matcher(associations, config_base_path)?)),
    None => Ok(None),
  }
}

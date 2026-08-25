use anyhow::Result;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use crate::environment::CanonicalizedPathBuf;
use crate::patterns::get_patterns_as_glob_matcher;
use crate::resolution::PluginWithConfig;
use crate::utils::GlobMatcher;
use crate::utils::GlobMatchesDetail;
use crate::utils::get_lowercase_file_extension;
use crate::utils::get_lowercase_file_name;

#[derive(Default)]
pub struct PluginNameResolutionMaps {
  extension_to_plugin_names_map: HashMap<String, Vec<String>>,
  file_name_to_plugin_names_map: HashMap<String, Vec<String>>,
  /// Associations matchers ordered by precedence.
  association_matchers: Vec<(String, Rc<GlobMatcher>)>,
  /// Associations matchers in a map.
  association_matchers_map: HashMap<String, Rc<GlobMatcher>>,
  /// Maps a file's shebang line to a file extension so extensionless scripts
  /// can be routed to a plugin.
  shebang_to_extension: HashMap<String, String>,
}

impl PluginNameResolutionMaps {
  pub fn from_plugins<'a>(
    plugins: impl Iterator<Item = &'a PluginWithConfig>,
    config_base_path: &CanonicalizedPathBuf,
    shebangs: Option<&IndexMap<String, String>>,
  ) -> Result<Self> {
    let mut plugin_name_maps = PluginNameResolutionMaps::default();
    if let Some(shebangs) = shebangs {
      for (shebang, extension) in shebangs {
        // extensions are stored lowercased and without a leading dot so they
        // resolve the same way as a real file extension
        let extension = extension.trim_start_matches('.').to_lowercase();
        plugin_name_maps.shebang_to_extension.insert(shebang.trim().to_string(), extension);
      }
    }
    for plugin in plugins {
      let plugin_name = plugin.name();

      for extension in &plugin.file_matching.file_extensions {
        plugin_name_maps
          .extension_to_plugin_names_map
          .entry(extension.to_lowercase())
          .or_default()
          .push(plugin_name.to_string());
      }
      for file_name in &plugin.file_matching.file_names {
        plugin_name_maps
          .file_name_to_plugin_names_map
          .entry(file_name.to_lowercase())
          .or_default()
          .push(plugin_name.to_string());
      }

      if let Some(matcher) = get_plugin_association_glob_matcher(plugin, config_base_path)? {
        let matcher = Rc::new(matcher);
        plugin_name_maps.association_matchers.push((plugin_name.to_string(), matcher.clone()));
        plugin_name_maps.association_matchers_map.insert(plugin_name.to_string(), matcher);
      }
    }
    Ok(plugin_name_maps)
  }

  pub fn get_plugin_names_from_file_path(&self, file_path: &Path) -> Vec<String> {
    let mut plugin_names = Vec::new();

    for (plugin_name, matcher) in self.association_matchers.iter() {
      if matcher.matches(file_path) {
        plugin_names.push(plugin_name.to_owned());
      }
    }

    if !plugin_names.is_empty() {
      return plugin_names;
    }

    if let Some(file_name) = get_lowercase_file_name(file_path)
      && let Some(plugin_names) = self.file_name_to_plugin_names_map.get(&file_name)
    {
      for plugin_name in plugin_names {
        if self.is_not_associations_excluded(plugin_name, file_path) {
          return vec![plugin_name.clone()];
        }
      }
    }

    if let Some(ext) = get_lowercase_file_extension(file_path)
      && let Some(plugin_names) = self.extension_to_plugin_names_map.get(&ext)
    {
      for plugin_name in plugin_names {
        if self.is_not_associations_excluded(plugin_name, file_path) {
          return vec![plugin_name.clone()];
        }
      }
    }

    plugin_names
  }

  pub fn has_shebang_mappings(&self) -> bool {
    !self.shebang_to_extension.is_empty()
  }

  /// Resolves plugins for an extensionless file based on its first line (the
  /// shebang). The shebang is looked up in the configured mapping to get an
  /// extension, then the file is resolved as if it had that extension.
  pub fn get_plugin_names_from_shebang_line(&self, file_path: &Path, first_line: &str) -> Vec<String> {
    let shebang = first_line.trim();
    if !shebang.starts_with("#!") {
      return Vec::new();
    }
    match self.shebang_to_extension.get(shebang) {
      Some(extension) => {
        let mut file_name = match file_path.file_name() {
          Some(file_name) => file_name.to_os_string(),
          None => return Vec::new(),
        };
        file_name.push(".");
        file_name.push(extension);
        self.get_plugin_names_from_file_path(&file_path.with_file_name(file_name))
      }
      None => Vec::new(),
    }
  }

  fn is_not_associations_excluded(&self, plugin_name: &str, file_path: &Path) -> bool {
    // `associations` add to the plugin's default file matching, so a plugin
    // keeps matching by its default extension/file name unless a negated
    // association pattern explicitly excludes the file
    match self.association_matchers_map.get(plugin_name) {
      Some(matcher) => matcher.matches_detail(file_path) != GlobMatchesDetail::Excluded,
      None => true,
    }
  }
}

fn get_plugin_association_glob_matcher(plugin: &PluginWithConfig, config_base_path: &CanonicalizedPathBuf) -> Result<Option<GlobMatcher>> {
  match plugin.associations.as_deref() {
    Some(associations) => Ok(Some(get_patterns_as_glob_matcher(associations, config_base_path)?)),
    None => Ok(None),
  }
}

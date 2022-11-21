use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::environment::CanonicalizedPathBuf;
use crate::patterns::get_patterns_as_glob_matcher;
use crate::utils::get_lowercase_file_extension;
use crate::utils::get_lowercase_file_name;
use crate::utils::GlobMatcher;
use crate::utils::GlobMatchesDetail;

use super::Plugin;

#[derive(Default)]
pub struct PluginNameResolutionMaps {
  extension_to_plugin_names_map: HashMap<String, Vec<String>>,
  file_name_to_plugin_names_map: HashMap<String, Vec<String>>,
  /// Associations matchers ordered by precedence.
  association_matchers: Vec<(String, Arc<GlobMatcher>)>,
  /// Associations matchers in a map.
  association_matchers_map: HashMap<String, Arc<GlobMatcher>>,
}

impl PluginNameResolutionMaps {
  pub fn from_plugins(plugins: &[Box<dyn Plugin>], config_base_path: &CanonicalizedPathBuf) -> Result<Self> {
    let mut plugin_name_maps = PluginNameResolutionMaps::default();
    for plugin in plugins {
      let plugin_name = plugin.name();

      for extension in plugin.file_extensions() {
        plugin_name_maps
          .extension_to_plugin_names_map
          .entry(extension.to_lowercase())
          .or_default()
          .push(plugin_name.to_string());
      }
      for file_name in plugin.file_names() {
        plugin_name_maps
          .file_name_to_plugin_names_map
          .entry(file_name.to_lowercase())
          .or_default()
          .push(plugin_name.to_string());
      }

      if let Some(matcher) = get_plugin_association_glob_matcher(&**plugin, config_base_path)? {
        let matcher = Arc::new(matcher);
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

    if let Some(file_name) = get_lowercase_file_name(file_path) {
      if let Some(plugin_names) = self.file_name_to_plugin_names_map.get(&file_name) {
        for plugin_name in plugin_names {
          if self.is_not_associations_excluded(plugin_name, file_path) {
            return vec![plugin_name.clone()];
          }
        }
      }
    }

    if let Some(ext) = get_lowercase_file_extension(file_path) {
      if let Some(plugin_names) = self.extension_to_plugin_names_map.get(&ext) {
        for plugin_name in plugin_names {
          if self.is_not_associations_excluded(plugin_name, file_path) {
            return vec![plugin_name.clone()];
          }
        }
      }
    }

    plugin_names
  }

  fn is_not_associations_excluded(&self, plugin_name: &str, file_path: &Path) -> bool {
    if let Some(matcher) = self.association_matchers_map.get(plugin_name) {
      matcher.has_only_excludes() && matcher.matches_detail(file_path) == GlobMatchesDetail::NotMatched
    } else {
      true
    }
  }
}

fn get_plugin_association_glob_matcher(plugin: &dyn Plugin, config_base_path: &CanonicalizedPathBuf) -> Result<Option<GlobMatcher>> {
  Ok(if let Some(associations) = plugin.get_config().0.associations.as_ref() {
    Some(get_patterns_as_glob_matcher(associations, config_base_path)?)
  } else {
    None
  })
}

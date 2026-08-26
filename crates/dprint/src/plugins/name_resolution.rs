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
use crate::utils::get_shebang_line;
use crate::utils::is_shebang_prefix_match;

#[derive(Default)]
pub struct PluginNameResolutionMaps {
  extension_to_plugin_names_map: HashMap<String, Vec<String>>,
  file_name_to_plugin_names_map: HashMap<String, Vec<String>>,
  /// Associations matchers ordered by precedence.
  association_matchers: Vec<(String, Rc<GlobMatcher>)>,
  /// Associations matchers in a map.
  association_matchers_map: HashMap<String, Rc<GlobMatcher>>,
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

  /// Resolves plugins for a file, falling back to its shebang line when it's
  /// extensionless and no plugin matched by path. `file_bytes_start` only
  /// needs to contain the start of the file.
  pub fn get_plugin_names_from_file_path_and_bytes(&self, file_path: &Path, file_bytes_start: &[u8]) -> Vec<String> {
    let plugin_names = self.get_plugin_names_from_file_path(file_path);
    if !plugin_names.is_empty() {
      return plugin_names;
    }
    self.get_plugin_names_from_shebang(file_path, file_bytes_start)
  }

  /// Whether the file might be resolved by its shebang line. This is the case
  /// when shebang mappings are configured and the file has no extension.
  pub fn may_match_shebang(&self, file_path: &Path) -> bool {
    !self.shebang_to_extension.is_empty() && file_path.extension().is_none()
  }

  /// Resolves plugins for an extensionless file based on its first line (the
  /// shebang). The shebang is looked up in the configured mapping to get an
  /// extension, then the plugins for that extension are resolved. Association
  /// patterns are evaluated against the real file path.
  ///
  /// A configured shebang matches when the file's shebang line equals it or
  /// starts with it followed by whitespace, so `#!/usr/bin/env deno run` matches
  /// `#!/usr/bin/env deno run --allow-read` but not `#!/usr/bin/env deno runtest`.
  pub fn get_plugin_names_from_shebang(&self, file_path: &Path, file_bytes_start: &[u8]) -> Vec<String> {
    if !self.may_match_shebang(file_path) {
      return Vec::new();
    }
    let Some(shebang) = get_shebang_line(file_bytes_start) else {
      return Vec::new();
    };
    let Some(extension) = self
      .shebang_to_extension
      .iter()
      .find(|(configured_shebang, _)| is_shebang_prefix_match(shebang, configured_shebang))
      .map(|(_, extension)| extension)
    else {
      return Vec::new();
    };
    let Some(plugin_names) = self.extension_to_plugin_names_map.get(extension) else {
      return Vec::new();
    };
    for plugin_name in plugin_names {
      if self.is_not_associations_excluded(plugin_name, file_path) {
        return vec![plugin_name.clone()];
      }
    }
    Vec::new()
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

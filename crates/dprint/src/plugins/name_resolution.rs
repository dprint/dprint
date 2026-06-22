use anyhow::Result;
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
  association_matchers_map: HashMap<String, PluginAssociationMatcher>,
}

struct PluginAssociationMatcher {
  matcher: Rc<GlobMatcher>,
  /// Whether the plugin still matches files by its default file extensions and
  /// file names. This is `false` only when `associations` replaces the defaults
  /// (i.e. it has at least one non-negated pattern). `appendAssociations` keeps
  /// the defaults and adds to them.
  keep_default_matching: bool,
}

impl PluginNameResolutionMaps {
  pub fn from_plugins<'a>(plugins: impl Iterator<Item = &'a PluginWithConfig>, config_base_path: &CanonicalizedPathBuf) -> Result<Self> {
    let mut plugin_name_maps = PluginNameResolutionMaps::default();
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

      if let Some((matcher, keep_default_matching)) = get_plugin_association_glob_matcher(plugin, config_base_path)? {
        let matcher = Rc::new(matcher);
        plugin_name_maps.association_matchers.push((plugin_name.to_string(), matcher.clone()));
        plugin_name_maps.association_matchers_map.insert(
          plugin_name.to_string(),
          PluginAssociationMatcher {
            matcher,
            keep_default_matching,
          },
        );
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

  fn is_not_associations_excluded(&self, plugin_name: &str, file_path: &Path) -> bool {
    match self.association_matchers_map.get(plugin_name) {
      // keep matching by default extension/file name unless `associations`
      // replaced the defaults, but still honour any negated association patterns
      Some(association) => association.keep_default_matching && association.matcher.matches_detail(file_path) != GlobMatchesDetail::Excluded,
      None => true,
    }
  }
}

/// Builds the combined glob matcher for a plugin's `associations` and
/// `appendAssociations`, returning it along with whether the plugin keeps
/// matching its default file extensions and file names.
///
/// `associations` replaces the default matching when it specifies a pattern to
/// match (i.e. anything other than only negated excludes), whereas
/// `appendAssociations` only ever adds and always keeps the defaults.
fn get_plugin_association_glob_matcher(plugin: &PluginWithConfig, config_base_path: &CanonicalizedPathBuf) -> Result<Option<(GlobMatcher, bool)>> {
  let associations = plugin.associations.as_deref();
  let append_associations = plugin.append_associations.as_deref();
  match (associations, append_associations) {
    (None, None) => Ok(None),
    (Some(associations), None) => {
      let matcher = get_patterns_as_glob_matcher(associations, config_base_path)?;
      let keep_default_matching = matcher.has_only_excludes();
      Ok(Some((matcher, keep_default_matching)))
    }
    (None, Some(append_associations)) => {
      let matcher = get_patterns_as_glob_matcher(append_associations, config_base_path)?;
      Ok(Some((matcher, true)))
    }
    (Some(associations), Some(append_associations)) => {
      let keep_default_matching = get_patterns_as_glob_matcher(associations, config_base_path)?.has_only_excludes();
      let patterns = associations.iter().chain(append_associations.iter()).cloned().collect::<Vec<_>>();
      let matcher = get_patterns_as_glob_matcher(&patterns, config_base_path)?;
      Ok(Some((matcher, keep_default_matching)))
    }
  }
}

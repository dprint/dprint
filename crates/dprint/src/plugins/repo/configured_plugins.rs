use std::collections::HashSet;

use super::InfoFilePluginInfo;
use crate::utils::PathSource;

/// The plugins a config file has, used for telling which of the info file's
/// plugins it already covers.
#[derive(Default)]
pub struct ConfiguredPlugins {
  names: HashSet<String>,
  sources: Vec<PathSource>,
}

impl ConfiguredPlugins {
  #[cfg(test)]
  pub fn from_names(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
    Self {
      names: names.into_iter().map(Into::into).collect(),
      sources: Vec::new(),
    }
  }

  /// Adds a plugin from the config file. The name is what the plugin reports
  /// about itself, which is only known when the plugin could be resolved.
  pub fn add(&mut self, source: PathSource, name: Option<String>) {
    self.names.extend(name);
    self.sources.push(source);
  }

  /// Whether the config file has the provided info file plugin.
  ///
  /// The name a plugin reports about itself isn't necessarily the name it has
  /// in the info file (ex. `dprint_plugin_malva` is `g-plane/malva` there), so
  /// the config file's entries are also matched on where the plugin comes from.
  pub fn has(&self, plugin: &InfoFilePluginInfo) -> bool {
    self.names.contains(&plugin.name) || self.sources.iter().any(|source| is_plugin_source(plugin, source))
  }
}

fn is_plugin_source(plugin: &InfoFilePluginInfo, source: &PathSource) -> bool {
  match source {
    PathSource::Npm(source) => plugin
      .npm
      .as_ref()
      .is_some_and(|npm| npm.name == source.specifier.name && npm.path.as_ref().is_none_or(|path| *path == source.specifier.path)),
    PathSource::Remote(source) => source.url.as_str() == plugin.url,
    PathSource::Local(_) => false,
  }
}

#[cfg(test)]
mod test {
  use url::Url;

  use super::*;
  use crate::plugins::PluginNpmInfo;
  use crate::utils::NpmPathSource;
  use crate::utils::NpmSpecifier;

  #[test]
  fn should_match_on_name() {
    let plugins = ConfiguredPlugins::from_names(["dprint-plugin-json"]);
    assert!(plugins.has(&info_plugin("dprint-plugin-json", "https://plugins.dprint.dev/json-0.25.1.wasm", "0.25.1")));
    assert!(!plugins.has(&info_plugin("g-plane/malva", "https://plugins.dprint.dev/g-plane/malva-v0.16.0.wasm", "0.16.0")));
  }

  #[test]
  fn should_match_on_npm_package_when_the_names_differ() {
    let mut plugins = ConfiguredPlugins::default();
    plugins.add(npm_source("dprint-plugin-malva", "plugin.wasm"), Some("dprint_plugin_malva".to_string()));

    let mut malva = info_plugin("g-plane/malva", "https://plugins.dprint.dev/g-plane/malva-v0.16.0.wasm", "0.16.0");
    malva.npm = Some(PluginNpmInfo {
      name: "dprint-plugin-malva".to_string(),
      path: None,
    });
    assert!(plugins.has(&malva));

    let mut markup = info_plugin("g-plane/markup_fmt", "https://plugins.dprint.dev/g-plane/markup_fmt-v0.27.5.wasm", "0.27.5");
    markup.npm = Some(PluginNpmInfo {
      name: "dprint-plugin-markup".to_string(),
      path: None,
    });
    assert!(!plugins.has(&markup));
  }

  #[test]
  fn should_match_on_npm_path_when_a_package_holds_several_plugins() {
    let mut plugins = ConfiguredPlugins::default();
    plugins.add(npm_source("plugins", "json/plugin.wasm"), None);

    let mut json = info_plugin("json", "https://example.com/json-1.0.0.wasm", "1.0.0");
    json.npm = Some(PluginNpmInfo {
      name: "plugins".to_string(),
      path: Some("json/plugin.wasm".to_string()),
    });
    assert!(plugins.has(&json));

    let mut toml = info_plugin("toml", "https://example.com/toml-1.0.0.wasm", "1.0.0");
    toml.npm = Some(PluginNpmInfo {
      name: "plugins".to_string(),
      path: Some("toml/plugin.wasm".to_string()),
    });
    assert!(!plugins.has(&toml));
  }

  #[test]
  fn should_match_on_url_when_the_names_differ() {
    let mut plugins = ConfiguredPlugins::default();
    plugins.add(
      PathSource::new_remote(Url::parse("https://example.com/malva-v0.16.0.wasm").unwrap()),
      Some("dprint_plugin_malva".to_string()),
    );
    assert!(plugins.has(&info_plugin("g-plane/malva", "https://example.com/malva-v0.16.0.wasm", "0.16.0")));
    // only the same url is known to be the same plugin
    assert!(!plugins.has(&info_plugin("g-plane/malva", "https://example.com/malva-v0.17.0.wasm", "0.17.0")));
  }

  fn info_plugin(name: &str, url: &str, version: &str) -> InfoFilePluginInfo {
    InfoFilePluginInfo {
      name: name.to_string(),
      version: version.to_string(),
      url: url.to_string(),
      config_key: None,
      file_extensions: Vec::new(),
      file_names: Vec::new(),
      config_excludes: Vec::new(),
      checksum: None,
      additive: false,
      never_preselect: false,
      npm: None,
      default_config: None,
      config_items: Vec::new(),
    }
  }

  fn npm_source(name: &str, path: &str) -> PathSource {
    PathSource::Npm(NpmPathSource {
      specifier: NpmSpecifier {
        name: name.to_string(),
        version: Some("1.0.0".to_string()),
        path: path.to_string(),
      },
      base_dir: None,
    })
  }
}

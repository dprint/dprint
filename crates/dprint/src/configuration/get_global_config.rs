use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigurationDiagnostic;
use dprint_core::configuration::GlobalConfiguration;

use super::ConfigMap;
use super::ConfigMapValue;

pub enum GlobalConfigDiagnostic {
  UnknownProperty(ConfigurationDiagnostic),
  Other(ConfigurationDiagnostic),
}

impl std::fmt::Display for GlobalConfigDiagnostic {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      GlobalConfigDiagnostic::UnknownProperty(diagnostic) => diagnostic.fmt(f),
      GlobalConfigDiagnostic::Other(diagnostic) => diagnostic.fmt(f),
    }
  }
}

pub struct GlobalConfigurationResult {
  pub config: GlobalConfiguration,
  pub diagnostics: Vec<GlobalConfigDiagnostic>,
}

pub fn get_global_config(mut config_map: ConfigMap) -> GlobalConfigurationResult {
  let mut diagnostics = Vec::new();

  // ignore this property
  config_map.remove("$schema");

  // now get and resolve the global config
  let mut global_config = get_global_config_from_config_map(&mut diagnostics, config_map);
  let global_config_result = dprint_core::configuration::resolve_global_config(&mut global_config);
  diagnostics.extend(global_config_result.diagnostics.into_iter().map(GlobalConfigDiagnostic::Other));

  let unknown_property_diagnostics = dprint_core::configuration::get_unknown_property_diagnostics(global_config);
  diagnostics.extend(unknown_property_diagnostics.into_iter().map(GlobalConfigDiagnostic::UnknownProperty));

  return GlobalConfigurationResult {
    config: global_config_result.config,
    diagnostics,
  };

  fn get_global_config_from_config_map(diagnostics: &mut Vec<GlobalConfigDiagnostic>, config_map: ConfigMap) -> ConfigKeyMap {
    // at this point, there should only be key values inside the hash map
    let mut global_config = ConfigKeyMap::new();

    for (key, value) in config_map.into_iter() {
      if let ConfigMapValue::KeyValue(value) = value {
        global_config.insert(key, value);
      } else {
        diagnostics.push(GlobalConfigDiagnostic::UnknownProperty(ConfigurationDiagnostic {
          property_name: key,
          message: "Unexpected non-string, boolean, or int property".to_string(),
        }));
      }
    }

    global_config
  }
}

#[cfg(test)]
mod tests {
  use dprint_core::configuration::NewLineKind;

  use super::*;
  use crate::configuration::ConfigMap;

  #[test]
  fn should_get_global_config() {
    let mut config_map = ConfigMap::new();
    config_map.insert(String::from("lineWidth"), ConfigMapValue::from_i32(80));
    config_map.insert(String::from("useTabs"), ConfigMapValue::from_bool(true));
    config_map.insert(String::from("indentWidth"), ConfigMapValue::from_i32(2));
    config_map.insert(String::from("newLineKind"), ConfigMapValue::from_str("crlf"));
    assert_result(
      config_map,
      GlobalConfiguration {
        line_width: Some(80),
        use_tabs: Some(true),
        indent_width: Some(2),
        new_line_kind: Some(NewLineKind::CarriageReturnLineFeed),
      },
      &[],
    );
  }

  #[test]
  fn should_diagnostic_on_unexpected_object_properties() {
    let mut config_map = ConfigMap::new();
    config_map.insert(String::from("test"), ConfigMapValue::PluginConfig(Default::default()));
    assert_result(
      config_map,
      GlobalConfiguration {
        line_width: None,
        use_tabs: None,
        indent_width: None,
        new_line_kind: None,
      },
      &["Unexpected non-string, boolean, or int property (test)"],
    );
  }

  #[test]
  fn should_diagnostic_on_unknown_props_and_values() {
    let mut config_map = ConfigMap::new();
    config_map.insert(String::from("lineWidth"), ConfigMapValue::from_str("test"));
    config_map.insert(String::from("unknownProperty"), ConfigMapValue::from_i32(80));
    assert_result(
      config_map,
      GlobalConfiguration {
        line_width: None,
        use_tabs: None,
        indent_width: None,
        new_line_kind: None,
      },
      &[
        "invalid digit found in string (lineWidth)",
        "Unknown property in configuration (unknownProperty)",
      ],
    );
  }

  #[test]
  fn should_ignore_schema_property() {
    let mut config_map = ConfigMap::new();
    config_map.insert(String::from("$schema"), ConfigMapValue::from_str("test"));
    assert_result(
      config_map,
      GlobalConfiguration {
        line_width: None,
        use_tabs: None,
        indent_width: None,
        new_line_kind: None,
      },
      &[],
    );
  }

  #[track_caller]
  fn assert_result(config_map: ConfigMap, global_config: GlobalConfiguration, diagnostics: &[&str]) {
    let result = get_global_config(config_map);
    assert_eq!(result.config, global_config);
    assert_eq!(
      result.diagnostics.into_iter().map(|d| d.to_string()).collect::<Vec<_>>(),
      diagnostics.into_iter().map(|d| d.to_string()).collect::<Vec<_>>()
    );
  }
}

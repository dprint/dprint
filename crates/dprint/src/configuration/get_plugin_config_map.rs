use anyhow::Result;
use anyhow::bail;

use super::ConfigMap;
use super::ConfigMapValue;
use super::RawPluginConfig;
use crate::plugins::PluginWrapper;

pub fn get_plugin_config_map(plugin: &PluginWrapper, config_map: &mut ConfigMap) -> Result<RawPluginConfig> {
  match get_plugin_config_map_inner(plugin, config_map) {
    Ok(result) => Ok(result),
    Err(err) => bail!("Error initializing from configuration file. {:#}", err),
  }
}

fn get_plugin_config_map_inner(plugin: &PluginWrapper, config_map: &mut ConfigMap) -> Result<RawPluginConfig> {
  let config_key = &plugin.info().config_key;

  if let Some(plugin_config) = config_map.shift_remove(config_key) {
    if let ConfigMapValue::PluginConfig(plugin_config) = plugin_config {
      Ok(plugin_config)
    } else {
      bail!("Expected the configuration property '{}' to be an object.", config_key)
    }
  } else {
    Ok(Default::default())
  }
}

#[cfg(test)]
mod tests {
  use crate::plugins::TestPlugin;
  use dprint_core::configuration::ConfigKeyMap;
  use dprint_core::configuration::ConfigKeyValue;

  use super::super::ConfigMap;
  use super::super::ConfigMapValue;
  use super::*;

  #[test]
  fn should_get_config_for_plugin() {
    let mut config_map = ConfigMap::new();
    let ts_plugin = RawPluginConfig {
      associations: None,
      locked: false,
      properties: ConfigKeyMap::from([("lineWidth".to_string(), ConfigKeyValue::from_i32(40))]),
    };
    config_map.insert(String::from("lineWidth"), ConfigMapValue::from_i32(80));
    config_map.insert(String::from("typescript"), ConfigMapValue::PluginConfig(ts_plugin.clone()));
    let plugin = PluginWrapper::new(Box::new(create_plugin()));
    let result = get_plugin_config_map(&plugin, &mut config_map).unwrap();
    assert_eq!(result, ts_plugin);
    assert_eq!(config_map.contains_key("typescript"), false);
  }

  #[test]
  fn should_error_plugin_key_is_not_object() {
    let mut config_map = ConfigMap::new();
    config_map.insert(String::from("lineWidth"), ConfigMapValue::from_i32(80));
    config_map.insert(String::from("typescript"), ConfigMapValue::from_str(""));
    assert_errors(&mut config_map, "Expected the configuration property 'typescript' to be an object.");
  }

  fn assert_errors(config_map: &mut ConfigMap, message: &str) {
    let plugin = PluginWrapper::new(Box::new(create_plugin()));
    let result = get_plugin_config_map(&plugin, config_map);
    assert_eq!(
      result.err().unwrap().to_string(),
      format!("Error initializing from configuration file. {}", message)
    );
  }

  fn create_plugin() -> TestPlugin {
    TestPlugin::new("dprint-plugin-typescript", "typescript", vec![".ts"], vec![])
  }
}

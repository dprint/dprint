use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigKeyValue;
use std::collections::HashMap;

/// Unresolved plugin configuration.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RawPluginConfig {
  pub associations: Option<Vec<String>>,
  pub locked: bool,
  pub properties: ConfigKeyMap,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ConfigMapValue {
  KeyValue(ConfigKeyValue),
  PluginConfig(RawPluginConfig),
  Vec(Vec<String>),
}

impl ConfigMapValue {
  pub fn from_i32(value: i32) -> ConfigMapValue {
    ConfigMapValue::KeyValue(ConfigKeyValue::from_i32(value))
  }

  #[cfg(test)]
  pub fn from_str(value: &str) -> ConfigMapValue {
    ConfigMapValue::KeyValue(ConfigKeyValue::from_str(value))
  }

  pub fn from_bool(value: bool) -> ConfigMapValue {
    ConfigMapValue::KeyValue(ConfigKeyValue::from_bool(value))
  }
}

pub type ConfigMap = HashMap<String, ConfigMapValue>;

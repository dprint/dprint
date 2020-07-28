use std::collections::HashMap;
use dprint_core::configuration::{ConfigKeyMap, ConfigKeyValue};

#[derive(Clone, PartialEq, Debug)]
pub enum ConfigMapValue {
    KeyValue(ConfigKeyValue),
    HashMap(ConfigKeyMap),
    Vec(Vec<String>)
}

impl ConfigMapValue {
    pub fn from_i32(value: i32) -> ConfigMapValue {
        ConfigMapValue::KeyValue(ConfigKeyValue::from_i32(value))
    }

    pub fn from_str(value: &str) -> ConfigMapValue {
        ConfigMapValue::KeyValue(ConfigKeyValue::from_str(value))
    }

    pub fn from_bool(value: bool) -> ConfigMapValue {
        ConfigMapValue::KeyValue(ConfigKeyValue::from_bool(value))
    }
}

pub type ConfigMap = HashMap<String, ConfigMapValue>;

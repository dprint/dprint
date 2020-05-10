use std::collections::HashMap;

#[derive(Clone, PartialEq, Debug)]
pub enum ConfigMapValue {
    String(String),
    HashMap(HashMap<String, String>),
    Vec(Vec<String>)
}

pub type ConfigMap = HashMap<String, ConfigMapValue>;

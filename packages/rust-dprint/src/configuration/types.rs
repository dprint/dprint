use std::collections::HashMap;

#[derive(Clone, PartialEq, Debug)]
pub enum StringOrHashMap {
    String(String),
    HashMap(HashMap<String, String>)
}

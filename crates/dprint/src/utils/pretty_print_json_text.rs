use serde_json::Value;
use std::collections::BTreeMap;

use anyhow::Result;

pub fn pretty_print_json_text(text: &str) -> Result<String> {
  // use a BTreeMap in order to serialize the keys in order
  let key_values: BTreeMap<String, Value> = serde_json::from_str(text)?;
  Ok(serde_json::to_string_pretty(&key_values)?)
}

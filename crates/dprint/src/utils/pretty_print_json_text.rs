use serde_json::Value;
use std::collections::BTreeMap;

use dprint_core::types::ErrBox;

pub fn pretty_print_json_text(text: &str) -> Result<String, ErrBox> {
  // use a BTreeMap in order to serialize the keys in order
  let key_values: BTreeMap<String, Value> = serde_json::from_str(text)?;
  Ok(serde_json::to_string_pretty(&key_values)?)
}

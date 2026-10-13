use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use dprint_core::async_runtime::async_trait;
use url::Url;

use crate::environment::DownloadOptions;
use crate::environment::Environment;
use crate::plugins::FormatConfig;
use crate::plugins::PluginResolver;
use crate::plugins::parse_plugin_source_reference;
use crate::utils::PathSource;
use crate::utils::SettingsOption;
use crate::utils::SettingsOptionKind;

/// Loads the options a plugin can be configured with so `dprint init` can
/// offer to edit them.
#[async_trait(?Send)]
pub trait InitPluginOptionsLoader {
  /// The options of the plugin that a config file's entry is for (ex. its
  /// url), each with its default value and nothing to write.
  async fn load(&self, plugin_entry: &str) -> Result<Vec<SettingsOption>>;
}

/// Loads a plugin's options by downloading the plugin, which is what knows
/// where its config schema is and what its options default to.
pub struct PluginResolverOptionsLoader<TEnvironment: Environment> {
  environment: TEnvironment,
  plugin_resolver: Rc<PluginResolver<TEnvironment>>,
}

impl<TEnvironment: Environment> PluginResolverOptionsLoader<TEnvironment> {
  pub fn new(environment: TEnvironment, plugin_resolver: Rc<PluginResolver<TEnvironment>>) -> Self {
    Self { environment, plugin_resolver }
  }

  /// The plugin's config schema, which is best-effort because the options
  /// can still be listed from the plugin's resolved config without it.
  async fn load_schema(&self, schema_url: &str) -> Option<serde_json::Value> {
    if schema_url.is_empty() {
      return None;
    }
    let result = async {
      let url = Url::parse(schema_url)?;
      let (_, file) = self.environment.download_file_err_404(&url, DownloadOptions::default()).await?;
      Ok::<_, anyhow::Error>(serde_json::from_slice::<serde_json::Value>(&file.content)?)
    }
    .await;
    match result {
      Ok(schema) => Some(schema),
      Err(err) => {
        log_debug!(self.environment, "Failed getting the config schema at {}: {:#}", schema_url, err);
        None
      }
    }
  }
}

#[async_trait(?Send)]
impl<TEnvironment: Environment> InitPluginOptionsLoader for PluginResolverOptionsLoader<TEnvironment> {
  async fn load(&self, plugin_entry: &str) -> Result<Vec<SettingsOption>> {
    let base = PathSource::new_local(self.environment.cwd());
    let plugin_reference = parse_plugin_source_reference(plugin_entry, &base, &self.environment)?;
    let plugin = self.plugin_resolver.resolve_plugin(plugin_reference).await?;
    let schema_url = plugin.info().config_schema_url.clone();
    let initialized_plugin = plugin.initialize().await?;
    // what the plugin resolves no config to is what its options default to
    let resolved_config = initialized_plugin
      .resolved_config(Arc::new(FormatConfig {
        id: self.plugin_resolver.next_config_id(),
        plugin: Default::default(),
        global: Default::default(),
      }))
      .await?;
    let defaults = match serde_json::from_str::<serde_json::Value>(&resolved_config) {
      Ok(serde_json::Value::Object(defaults)) => defaults,
      _ => Default::default(),
    };
    let schema = self.load_schema(&schema_url).await;
    Ok(get_settings_options(schema.as_ref(), &defaults))
  }
}

/// Sets the values to write on the options. A value for an option that isn't
/// known (ex. the plugin no longer has it) is kept by adding an option for it.
pub fn apply_config_to_settings_options(options: &mut Vec<SettingsOption>, config: &serde_json::Map<String, serde_json::Value>) {
  for (name, value) in config {
    match options.iter_mut().find(|option| &option.name == name) {
      Some(option) => option.value = Some(value.clone()),
      None => options.push(SettingsOption {
        name: name.clone(),
        kind: kind_of_value(value),
        default_value: None,
        value: Some(value.clone()),
        description: None,
      }),
    }
  }
}

/// The options that have a value to write.
pub fn get_config_from_settings_options(options: &[SettingsOption]) -> serde_json::Map<String, serde_json::Value> {
  options.iter().filter_map(|option| Some((option.name.clone(), option.value.clone()?))).collect()
}

/// The options described by a plugin's config schema, in the order it has
/// them, followed by any the plugin resolves that the schema doesn't mention.
/// The defaults are what the plugin resolved, since a schema doesn't always
/// say and can't know about a default that depends on another option.
fn get_settings_options(schema: Option<&serde_json::Value>, defaults: &serde_json::Map<String, serde_json::Value>) -> Vec<SettingsOption> {
  let mut options = Vec::new();
  let properties = schema.and_then(|schema| schema.get("properties")).and_then(|properties| properties.as_object());
  if let (Some(schema), Some(properties)) = (schema, properties) {
    for (name, property) in properties {
      if is_ignored_option(name) {
        continue;
      }
      let property = resolve_schema_ref(schema, property);
      options.push(SettingsOption {
        name: name.clone(),
        kind: kind_of_schema_property(schema, property),
        default_value: defaults.get(name).or_else(|| property.get("default")).cloned(),
        value: None,
        description: property.get("description").and_then(|value| value.as_str()).map(|text| text.to_string()),
      });
    }
  }
  for (name, default_value) in defaults {
    if is_ignored_option(name) || options.iter().any(|option| &option.name == name) {
      continue;
    }
    options.push(SettingsOption {
      name: name.clone(),
      kind: kind_of_value(default_value),
      default_value: Some(default_value.clone()),
      value: None,
      description: None,
    });
  }
  options
}

/// Properties of a plugin's config that aren't options of the plugin.
fn is_ignored_option(name: &str) -> bool {
  name == "$schema"
}

/// Follows references to definitions within the schema (ex. `#/definitions/quoteStyle`).
fn resolve_schema_ref<'a>(schema: &'a serde_json::Value, mut property: &'a serde_json::Value) -> &'a serde_json::Value {
  // bounded in case the definitions refer to each other
  for _ in 0..8 {
    let Some(pointer) = property
      .get("$ref")
      .and_then(|value| value.as_str())
      .and_then(|reference| reference.strip_prefix('#'))
    else {
      break;
    };
    match schema.pointer(pointer) {
      Some(definition) => property = definition,
      None => break,
    }
  }
  property
}

fn kind_of_schema_property(schema: &serde_json::Value, property: &serde_json::Value) -> SettingsOptionKind {
  if let Some(values) = enum_values_of_schema_property(schema, property) {
    let has = |value: bool| values.contains(&serde_json::Value::Bool(value));
    // only when it can be flipped between both, which isn't so for `[true]`
    return if has(true) && has(false) && values.iter().all(|value| value.is_boolean()) {
      SettingsOptionKind::Bool
    } else {
      SettingsOptionKind::Enum(values)
    };
  }
  let type_name = match property.get("type") {
    Some(serde_json::Value::String(name)) => Some(name.as_str()),
    // ex. ["string", "null"]
    Some(serde_json::Value::Array(names)) => names.iter().filter_map(|name| name.as_str()).find(|name| *name != "null"),
    _ => None,
  };
  match type_name {
    Some("boolean") => SettingsOptionKind::Bool,
    Some("number" | "integer") => SettingsOptionKind::Number,
    Some("string") => SettingsOptionKind::String,
    _ => SettingsOptionKind::ReadOnly,
  }
}

/// The values a property is limited to, when the schema lists them all with
/// `enum` or with alternatives that are each a constant.
fn enum_values_of_schema_property(schema: &serde_json::Value, property: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
  if let Some(values) = property.get("enum").and_then(|values| values.as_array()) {
    // no values is a schema that nothing matches, so go by its type instead
    return if values.is_empty() { None } else { Some(values.clone()) };
  }
  let alternatives = property
    .get("oneOf")
    .or_else(|| property.get("anyOf"))
    .and_then(|alternatives| alternatives.as_array())?;
  let mut values = Vec::new();
  for alternative in alternatives {
    let alternative = resolve_schema_ref(schema, alternative);
    if let Some(value) = alternative.get("const") {
      values.push(value.clone());
    } else if let Some(alternative_values) = alternative.get("enum").and_then(|values| values.as_array()) {
      values.extend(alternative_values.iter().cloned());
    } else if alternative.get("type").and_then(|name| name.as_str()) == Some("boolean") {
      values.extend([serde_json::Value::Bool(true), serde_json::Value::Bool(false)]);
    } else {
      // not something that can be chosen from a list
      return None;
    }
  }
  if values.is_empty() { None } else { Some(values) }
}

fn kind_of_value(value: &serde_json::Value) -> SettingsOptionKind {
  match value {
    serde_json::Value::Bool(_) => SettingsOptionKind::Bool,
    serde_json::Value::Number(_) => SettingsOptionKind::Number,
    serde_json::Value::String(_) => SettingsOptionKind::String,
    serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => SettingsOptionKind::ReadOnly,
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use pretty_assertions::assert_eq;
  use serde_json::json;

  fn option(name: &str, kind: SettingsOptionKind, default_value: Option<serde_json::Value>, description: Option<&str>) -> SettingsOption {
    SettingsOption {
      name: name.to_string(),
      kind,
      default_value,
      value: None,
      description: description.map(|text| text.to_string()),
    }
  }

  fn as_map(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value.as_object().unwrap().clone()
  }

  #[test]
  fn should_get_options_from_schema_with_defaults_from_the_plugin() {
    let schema = json!({
      "$schema": "http://json-schema.org/draft-07/schema#",
      "definitions": {
        "quoteStyle": {
          "description": "How to use quotes.",
          "type": "string",
          "default": "alwaysDouble",
          "oneOf": [
            { "const": "alwaysDouble", "description": "Always double." },
            { "const": "preferSingle", "description": "Prefer single." }
          ]
        },
        "useTabs": { "description": "Whether to use tabs.", "type": "boolean", "default": false }
      },
      "properties": {
        "$schema": { "type": "string" },
        "lineWidth": { "description": "The width.", "type": "number", "default": 120 },
        "useTabs": { "$ref": "#/definitions/useTabs" },
        "quoteStyle": { "$ref": "#/definitions/quoteStyle" },
        "newLineKind": { "type": "string", "enum": ["auto", "lf"] },
        "semiColons": { "oneOf": [{ "type": "boolean" }, { "const": "asi" }] },
        "ignoreComment": { "type": ["string", "null"] },
        "commands": { "type": "array" },
        "mixed": { "oneOf": [{ "type": "string" }, { "const": "x" }] }
      }
    });
    // the plugin resolved a line width other than what the schema says
    let defaults = as_map(json!({ "lineWidth": 80, "useTabs": false, "quoteStyle": "alwaysDouble", "notInSchema": "x" }));
    assert_eq!(
      get_settings_options(Some(&schema), &defaults),
      vec![
        option("lineWidth", SettingsOptionKind::Number, Some(json!(80)), Some("The width.")),
        option("useTabs", SettingsOptionKind::Bool, Some(json!(false)), Some("Whether to use tabs.")),
        option(
          "quoteStyle",
          SettingsOptionKind::Enum(vec![json!("alwaysDouble"), json!("preferSingle")]),
          Some(json!("alwaysDouble")),
          Some("How to use quotes.")
        ),
        option("newLineKind", SettingsOptionKind::Enum(vec![json!("auto"), json!("lf")]), None, None),
        option(
          "semiColons",
          SettingsOptionKind::Enum(vec![json!(true), json!(false), json!("asi")]),
          None,
          None
        ),
        option("ignoreComment", SettingsOptionKind::String, None, None),
        option("commands", SettingsOptionKind::ReadOnly, None, None),
        option("mixed", SettingsOptionKind::ReadOnly, None, None),
        option("notInSchema", SettingsOptionKind::String, Some(json!("x")), None),
      ]
    );
  }

  #[test]
  fn should_only_flip_between_values_a_schema_allows_both_of() {
    let schema = json!({
      "properties": {
        "both": { "enum": [true, false] },
        "onlyTrue": { "enum": [true] },
        "noValues": { "type": "boolean", "enum": [] },
        "noValuesOrType": { "enum": [] },
      }
    });
    let kinds = get_settings_options(Some(&schema), &Default::default())
      .into_iter()
      .map(|option| (option.name, option.kind))
      .collect::<Vec<_>>();
    assert_eq!(
      kinds,
      vec![
        ("both".to_string(), SettingsOptionKind::Bool),
        ("onlyTrue".to_string(), SettingsOptionKind::Enum(vec![json!(true)])),
        ("noValues".to_string(), SettingsOptionKind::Bool),
        ("noValuesOrType".to_string(), SettingsOptionKind::ReadOnly),
      ]
    );
  }

  #[test]
  fn should_get_options_from_the_resolved_config_without_a_schema() {
    let defaults = as_map(json!({ "ending": "formatted", "line_width": 120, "enabled": true, "items": [] }));
    assert_eq!(
      get_settings_options(None, &defaults),
      vec![
        option("ending", SettingsOptionKind::String, Some(json!("formatted")), None),
        option("line_width", SettingsOptionKind::Number, Some(json!(120)), None),
        option("enabled", SettingsOptionKind::Bool, Some(json!(true)), None),
        option("items", SettingsOptionKind::ReadOnly, Some(json!([])), None),
      ]
    );
  }

  #[test]
  fn should_apply_config_and_get_it_back() {
    let mut options = get_settings_options(None, &as_map(json!({ "ending": "formatted", "line_width": 120 })));
    let config = as_map(json!({ "line_width": 80, "removedOption": true }));
    apply_config_to_settings_options(&mut options, &config);
    assert_eq!(options[0].value, None);
    assert_eq!(options[1].value, Some(json!(80)));
    // kept even though the plugin doesn't have it
    assert_eq!(
      options[2],
      SettingsOption {
        name: "removedOption".to_string(),
        kind: SettingsOptionKind::Bool,
        default_value: None,
        value: Some(json!(true)),
        description: None,
      }
    );
    assert_eq!(get_config_from_settings_options(&options), config);
  }
}

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseConfigurationError(pub String);

impl std::fmt::Display for ParseConfigurationError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    format!("Found invalid value '{}'.", self.0).fmt(f)
  }
}

#[macro_export]
macro_rules! generate_str_to_from {
    ($enum_name:ident, $([$member_name:ident, $string_value:expr]),* ) => {
        impl std::str::FromStr for $enum_name {
            type Err = ParseConfigurationError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($string_value => Ok($enum_name::$member_name)),*,
                    _ => Err(ParseConfigurationError(String::from(s))),
                }
            }
        }

        impl std::string::ToString for $enum_name {
            fn to_string(&self) -> String {
                match self {
                    $($enum_name::$member_name => String::from($string_value)),*,
                }
            }
        }
    };
}

#[derive(Clone, PartialEq, Debug, Copy, Serialize, Deserialize)]
pub enum NewLineKind {
  /// Decide which newline kind to use based on the last newline in the file.
  #[serde(rename = "auto")]
  Auto,
  /// Use slash n new lines.
  #[serde(rename = "lf")]
  LineFeed,
  /// Use slash r slash n new lines.
  #[serde(rename = "crlf")]
  CarriageReturnLineFeed,
  /// Use the system standard (ex. crlf on Windows)
  #[serde(rename = "system")]
  System,
}

generate_str_to_from![
  NewLineKind,
  [Auto, "auto"],
  [LineFeed, "lf"],
  [CarriageReturnLineFeed, "crlf"],
  [System, "system"]
];

/// Represents a problem within the configuration.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationDiagnostic {
  /// The property name the problem occurred on.
  pub property_name: String,
  /// The diagnostic message that should be displayed to the user
  pub message: String,
}

impl std::fmt::Display for ConfigurationDiagnostic {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{} ({})", self.message, self.property_name)
  }
}

pub type ConfigKeyMap = HashMap<String, ConfigKeyValue>;

/// Creates a ConfigKeyMap from a series of string key value pairs.
pub fn parse_config_key_map(spec_config: &HashMap<String, String>) -> ConfigKeyMap {
  let mut key_map = HashMap::new();
  for (key, value) in spec_config {
    let new_value = match value.parse::<bool>() {
      Ok(value) => value.into(),
      Err(_) => match value.parse::<i32>() {
        Ok(value) => value.into(),
        Err(_) => value.clone().into(),
      },
    };
    key_map.insert(key.clone(), new_value);
  }
  key_map
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigKeyValue {
  String(String),
  Number(i32),
  Bool(bool),
}

impl ConfigKeyValue {
  pub fn from_i32(value: i32) -> ConfigKeyValue {
    ConfigKeyValue::Number(value)
  }

  pub fn from_str(value: &str) -> ConfigKeyValue {
    ConfigKeyValue::String(value.to_string())
  }

  pub fn from_bool(value: bool) -> ConfigKeyValue {
    ConfigKeyValue::Bool(value)
  }
}

impl From<i32> for ConfigKeyValue {
  fn from(item: i32) -> Self {
    ConfigKeyValue::from_i32(item)
  }
}

impl From<bool> for ConfigKeyValue {
  fn from(item: bool) -> Self {
    ConfigKeyValue::from_bool(item)
  }
}

impl From<String> for ConfigKeyValue {
  fn from(item: String) -> Self {
    ConfigKeyValue::from_str(&item)
  }
}

impl From<&str> for ConfigKeyValue {
  fn from(item: &str) -> Self {
    ConfigKeyValue::from_str(item)
  }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GlobalConfiguration {
  pub line_width: Option<u32>,
  pub use_tabs: Option<bool>,
  pub indent_width: Option<u8>,
  pub new_line_kind: Option<NewLineKind>,
}

pub const DEFAULT_GLOBAL_CONFIGURATION: DefaultGlobalConfiguration = DefaultGlobalConfiguration {
  line_width: 120,
  indent_width: 4,
  use_tabs: false,
  new_line_kind: NewLineKind::LineFeed,
};

pub struct DefaultGlobalConfiguration {
  pub line_width: u32,
  pub use_tabs: bool,
  pub indent_width: u8,
  pub new_line_kind: NewLineKind,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveConfigurationResult<T>
where
  T: Clone + Serialize,
{
  /// The configuration diagnostics.
  pub diagnostics: Vec<ConfigurationDiagnostic>,

  /// The configuration derived from the unresolved configuration
  /// that can be used to format a file.
  pub config: T,
}

pub struct ResolveGlobalConfigOptions {
  pub check_unknown_property_diagnostics: bool,
}

impl Default for ResolveGlobalConfigOptions {
  fn default() -> Self {
    Self {
      check_unknown_property_diagnostics: true,
    }
  }
}

/// Resolves a collection of key value pairs to a GlobalConfiguration.
pub fn resolve_global_config(config: ConfigKeyMap, options: &ResolveGlobalConfigOptions) -> ResolveConfigurationResult<GlobalConfiguration> {
  let mut config = config;
  let mut diagnostics = Vec::new();

  let resolved_config = GlobalConfiguration {
    line_width: get_nullable_value(&mut config, "lineWidth", &mut diagnostics),
    use_tabs: get_nullable_value(&mut config, "useTabs", &mut diagnostics),
    indent_width: get_nullable_value(&mut config, "indentWidth", &mut diagnostics),
    new_line_kind: get_nullable_value(&mut config, "newLineKind", &mut diagnostics),
  };

  if options.check_unknown_property_diagnostics {
    diagnostics.extend(get_unknown_property_diagnostics(config));
  }

  ResolveConfigurationResult {
    config: resolved_config,
    diagnostics,
  }
}

/// If the provided key exists, takes its value from the provided config and returns it.
/// If the provided key does not exist, it returns the default value.
/// Adds a diagnostic if there is any problem deserializing the value.
pub fn get_value<T>(config: &mut ConfigKeyMap, key: &'static str, default_value: T, diagnostics: &mut Vec<ConfigurationDiagnostic>) -> T
where
  T: std::str::FromStr,
  <T as std::str::FromStr>::Err: std::fmt::Display,
{
  get_nullable_value(config, key, diagnostics).unwrap_or(default_value)
}

/// If the provided key exists, takes its value from the provided config and returns it.
/// If the provided key does not exist, it returns None.
/// Adds a diagnostic if there is any problem deserializing the value.
pub fn get_nullable_value<T>(config: &mut ConfigKeyMap, key: &'static str, diagnostics: &mut Vec<ConfigurationDiagnostic>) -> Option<T>
where
  T: std::str::FromStr,
  <T as std::str::FromStr>::Err: std::fmt::Display,
{
  if let Some(raw_value) = config.remove(key) {
    // not exactly the best, but can't think of anything better at the moment
    let parsed_value = match raw_value {
      ConfigKeyValue::Bool(value) => value.to_string().parse::<T>(),
      ConfigKeyValue::Number(value) => value.to_string().parse::<T>(),
      ConfigKeyValue::String(value) => value.parse::<T>(),
    };
    match parsed_value {
      Ok(parsed_value) => Some(parsed_value),
      Err(message) => {
        diagnostics.push(ConfigurationDiagnostic {
          property_name: key.to_string(),
          message: message.to_string(),
        });
        None
      }
    }
  } else {
    None
  }
}

/// If it exists, moves over the configuration value over from the old key
/// to the new key and adds a diagnostic.
pub fn handle_renamed_config_property(config: &mut ConfigKeyMap, old_key: &'static str, new_key: &'static str, diagnostics: &mut Vec<ConfigurationDiagnostic>) {
  if let Some(raw_value) = config.remove(old_key) {
    if !config.contains_key(new_key) {
      config.insert(new_key.to_string(), raw_value);
    }
    diagnostics.push(ConfigurationDiagnostic {
      property_name: old_key.to_string(),
      message: format!("The configuration key was renamed to '{}'.", new_key),
    });
  }
}

/// Resolves the `NewLineKind` text from the provided file text and `NewLineKind`.
pub fn resolve_new_line_kind(file_text: &str, new_line_kind: NewLineKind) -> &'static str {
  match new_line_kind {
    NewLineKind::LineFeed => "\n",
    NewLineKind::CarriageReturnLineFeed => "\r\n",
    NewLineKind::Auto => {
      let mut found_slash_n = false;
      for c in file_text.as_bytes().iter().rev() {
        if found_slash_n {
          if c == &(b'\r') {
            return "\r\n";
          } else {
            return "\n";
          }
        }

        if c == &(b'\n') {
          found_slash_n = true;
        }
      }

      "\n"
    }
    NewLineKind::System => {
      if cfg!(windows) {
        "\r\n"
      } else {
        "\n"
      }
    }
  }
}

/// Gets a diagnostic for each remaining key value pair in the hash map.
///
/// This should be done last, so it swallows the hashmap.
pub fn get_unknown_property_diagnostics(config: ConfigKeyMap) -> Vec<ConfigurationDiagnostic> {
  let mut diagnostics = Vec::new();
  for (key, _) in config.iter() {
    diagnostics.push(ConfigurationDiagnostic {
      property_name: key.to_string(),
      message: "Unknown property in configuration.".to_string(),
    });
  }
  diagnostics
}

#[cfg(test)]
mod test {
  use super::*;
  use std::collections::HashMap;

  #[test]
  fn get_default_config_when_empty() {
    let config_result = resolve_global_config(HashMap::new(), &Default::default());
    let config = config_result.config;
    assert_eq!(config_result.diagnostics.len(), 0);
    assert_eq!(config.line_width, None);
    assert_eq!(config.indent_width, None);
    assert_eq!(config.new_line_kind.is_none(), true);
    assert_eq!(config.use_tabs, None);
  }

  #[test]
  fn get_values_when_filled() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("lineWidth"), ConfigKeyValue::from_i32(80));
    global_config.insert(String::from("indentWidth"), ConfigKeyValue::from_i32(8));
    global_config.insert(String::from("newLineKind"), ConfigKeyValue::from_str("crlf"));
    global_config.insert(String::from("useTabs"), ConfigKeyValue::from_bool(true));
    let config_result = resolve_global_config(global_config, &Default::default());
    let config = config_result.config;
    assert_eq!(config_result.diagnostics.len(), 0);
    assert_eq!(config.line_width, Some(80));
    assert_eq!(config.indent_width, Some(8));
    assert_eq!(config.new_line_kind == Some(NewLineKind::CarriageReturnLineFeed), true);
    assert_eq!(config.use_tabs, Some(true));
  }

  #[test]
  fn get_diagnostic_for_invalid_enum_config() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("newLineKind"), ConfigKeyValue::from_str("something"));
    let diagnostics = resolve_global_config(global_config, &Default::default()).diagnostics;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "Found invalid value 'something'.");
    assert_eq!(diagnostics[0].property_name, "newLineKind");
  }

  #[test]
  fn get_diagnostic_for_invalid_primitive() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("useTabs"), ConfigKeyValue::from_str("something"));
    let diagnostics = resolve_global_config(global_config, &Default::default()).diagnostics;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "provided string was not `true` or `false`");
    assert_eq!(diagnostics[0].property_name, "useTabs");
  }

  #[test]
  fn get_diagnostic_for_excess_property() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("something"), ConfigKeyValue::from_str("value"));
    let diagnostics = resolve_global_config(global_config, &Default::default()).diagnostics;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "Unknown property in configuration.");
    assert_eq!(diagnostics[0].property_name, "something");
  }

  #[test]
  fn no_diagnostic_for_excess_property_when_check_false() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("something"), ConfigKeyValue::from_str("value"));
    let diagnostics = resolve_global_config(
      global_config,
      &ResolveGlobalConfigOptions {
        check_unknown_property_diagnostics: false,
      },
    )
    .diagnostics;
    assert_eq!(diagnostics.len(), 0);
  }

  #[test]
  fn add_diagnostic_for_renamed_property() {
    let mut config = HashMap::new();
    let mut diagnostics = Vec::new();
    config.insert("oldProp".to_string(), ConfigKeyValue::from_str("value"));
    handle_renamed_config_property(&mut config, "oldProp", "newProp", &mut diagnostics);
    assert_eq!(config.len(), 1);
    assert_eq!(config.remove("newProp").unwrap(), ConfigKeyValue::from_str("value"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "The configuration key was renamed to 'newProp'.");
    assert_eq!(diagnostics[0].property_name, "oldProp");
  }

  #[test]
  fn add_diagnostic_for_renamed_property_when_already_exists() {
    let mut config = HashMap::new();
    let mut diagnostics = Vec::new();
    config.insert("oldProp".to_string(), ConfigKeyValue::from_str("new_value"));
    config.insert("newProp".to_string(), ConfigKeyValue::from_str("value"));
    handle_renamed_config_property(&mut config, "oldProp", "newProp", &mut diagnostics);
    assert_eq!(config.len(), 1);
    assert_eq!(config.remove("newProp").unwrap(), ConfigKeyValue::from_str("value"));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "The configuration key was renamed to 'newProp'.");
    assert_eq!(diagnostics[0].property_name, "oldProp");
  }
}

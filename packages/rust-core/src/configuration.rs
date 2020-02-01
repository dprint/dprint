use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseConfigurationError(pub String);

impl std::fmt::Display for ParseConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format!("Found invalid value '{}'.", self.0).fmt(f)
    }
}

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

#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
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
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationDiagnostic {
    /// The property name the problem occurred on.
    pub property_name: String,
    /// The diagnostic message that should be displayed to the user
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalConfiguration {
    pub indent_width: u8,
    pub line_width: u32,
    pub use_tabs: bool,
    pub new_line_kind: NewLineKind,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveConfigurationResult<T> where T : Clone + Serialize {
    /// The configuration diagnostics.
    pub diagnostics: Vec<ConfigurationDiagnostic>,

    /// The configuration derived from the unresolved configuration
    /// that can be used to format a file.
    pub config: T,
}

/// Resolves a collection of key value pairs to a GlobalConfiguration.
pub fn resolve_global_config(config: &HashMap<String, String>) -> ResolveConfigurationResult<GlobalConfiguration> {
    let mut diagnostics = Vec::new();
    let mut config = config.clone();

    let resolved_config = GlobalConfiguration {
        line_width: get_value(&mut config, "lineWidth", 120, &mut diagnostics),
        use_tabs: get_value(&mut config, "useTabs", false, &mut diagnostics),
        indent_width: get_value(&mut config, "indentWidth", 4, &mut diagnostics),
        new_line_kind: get_value(&mut config, "newLineKind", NewLineKind::Auto, &mut diagnostics),
    };

    for (key, _) in config.iter() {
        diagnostics.push(ConfigurationDiagnostic {
            property_name: String::from(key),
            message: format!("Unknown property in configuration: {}", key),
        });
    }

    ResolveConfigurationResult {
        config: resolved_config,
        diagnostics,
    }
}

/// If the provided key exists, takes its value from the provided config and returns it.
/// If the provided key does not exist, it returns the default value.
/// Adds a diagnostic if there is any problem deserializing the value.
pub fn get_value<T>(
    config: &mut HashMap<String, String>,
    key: &'static str,
    default_value: T,
    diagnostics: &mut Vec<ConfigurationDiagnostic>
) -> T where T : std::str::FromStr, <T as std::str::FromStr>::Err : std::fmt::Display {
    let value = if let Some(raw_value) = config.get(key) {
        if raw_value.trim().is_empty() {
            default_value
        } else {
            let parsed_value = raw_value.parse::<T>();
            match parsed_value {
                Ok(parsed_value) => parsed_value,
                Err(message) => {
                    diagnostics.push(ConfigurationDiagnostic {
                        property_name: String::from(key),
                        message: format!("Error parsing configuration value for '{}'. Message: {}", key, message)
                    });
                    default_value
                }
            }
        }
    } else {
        default_value
    };
    config.remove(key);
    return value;
}

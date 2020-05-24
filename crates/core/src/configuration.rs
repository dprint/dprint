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
pub struct ResolveConfigurationResult<T> where T : Clone + Serialize {
    /// The configuration diagnostics.
    pub diagnostics: Vec<ConfigurationDiagnostic>,

    /// The configuration derived from the unresolved configuration
    /// that can be used to format a file.
    pub config: T,
}

/// Resolves a collection of key value pairs to a GlobalConfiguration.
pub fn resolve_global_config(config: HashMap<String, String>) -> ResolveConfigurationResult<GlobalConfiguration> {
    let mut config = config;
    let mut diagnostics = Vec::new();

    let resolved_config = GlobalConfiguration {
        line_width: get_nullable_value(&mut config, "lineWidth", &mut diagnostics),
        use_tabs: get_nullable_value(&mut config, "useTabs", &mut diagnostics),
        indent_width: get_nullable_value(&mut config, "indentWidth", &mut diagnostics),
        new_line_kind: get_nullable_value(&mut config, "newLineKind", &mut diagnostics),
    };

    diagnostics.extend(get_unknown_property_diagnostics(config));

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
    get_nullable_value(config, key, diagnostics).unwrap_or(default_value)
}

fn get_nullable_value<T>(
    config: &mut HashMap<String, String>,
    key: &'static str,
    diagnostics: &mut Vec<ConfigurationDiagnostic>
) -> Option<T> where T : std::str::FromStr, <T as std::str::FromStr>::Err : std::fmt::Display {
    let value = if let Some(raw_value) = config.get(key) {
        if raw_value.trim().is_empty() {
            None
        } else {
            let parsed_value = raw_value.parse::<T>();
            match parsed_value {
                Ok(parsed_value) => Some(parsed_value),
                Err(message) => {
                    diagnostics.push(ConfigurationDiagnostic {
                        property_name: String::from(key),
                        message: format!("Error parsing configuration value for '{}'. Message: {}", key, message)
                    });
                    None
                }
            }
        }
    } else {
        None
    };
    config.remove(key);
    value
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
                    if c == &('\r' as u8) {
                        return "\r\n";
                    } else {
                        return "\n";
                    }
                }

                if c == &('\n' as u8) {
                    found_slash_n = true;
                }
            }

            return "\n";
        },
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
pub fn get_unknown_property_diagnostics(config: HashMap<String, String>) -> Vec<ConfigurationDiagnostic> {
    let mut diagnostics = Vec::new();
    for (key, _) in config.iter() {
        diagnostics.push(ConfigurationDiagnostic {
            property_name: String::from(key),
            message: format!("Unknown property in configuration: {}", key),
        });
    }
    diagnostics
}

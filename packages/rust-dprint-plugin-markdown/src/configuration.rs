use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use dprint_core::configuration::*;

/// Markdown formatting configuration builder.
///
/// # Example
///
/// ```
/// use dprint_plugin_markdown::configuration::*;
///
/// let config = ConfigurationBuilder::new()
///     .line_width(80)
///     .build();
/// ```
pub struct ConfigurationBuilder {
    config: HashMap<String, String>,
    global_config: Option<GlobalConfiguration>,
}

impl ConfigurationBuilder {
    /// Constructs a new configuration builder.
    pub fn new() -> ConfigurationBuilder {
        ConfigurationBuilder {
            config: HashMap::new(),
            global_config: None,
        }
    }

    /// Gets the final configuration that can be used to format a file.
    pub fn build(&self) -> Configuration {
        if let Some(global_config) = &self.global_config {
            resolve_config(&self.config, global_config).config
        } else {
            let global_config = resolve_global_config(&HashMap::new()).config;
            resolve_config(&self.config, &global_config).config
        }
    }

    /// Set the global configuration.
    pub fn global_config(&mut self, global_config: GlobalConfiguration) -> &mut Self {
        self.global_config = Some(global_config);
        self
    }

    /// The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.
    /// Default: 80
    pub fn line_width(&mut self, value: u32) -> &mut Self {
        self.insert("lineWidth", value)
    }

    /// Whether to use tabs (true) or spaces (false).
    /// Default: false
    pub fn use_tabs(&mut self, value: bool) -> &mut Self {
        self.insert("useTabs", value)
    }

    /// The number of columns for an indent.
    /// Default: 4
    pub fn indent_width(&mut self, value: u8) -> &mut Self {
        self.insert("indentWidth", value)
    }

    /// The kind of newline to use.
    /// Default: `NewLineKind::Auto`
    pub fn new_line_kind(&mut self, value: NewLineKind) -> &mut Self {
        self.insert("newLineKind", value)
    }

    #[cfg(test)]
    pub(super) fn get_inner_config(&self) -> HashMap<String, String> {
        self.config.clone()
    }

    fn insert<T>(&mut self, name: &str, value: T) -> &mut Self where T : std::string::ToString {
        self.config.insert(String::from(name), value.to_string());
        self
    }
}

/// Resolves configuration from a collection of key value strings.
///
/// # Example
///
/// ```
/// use std::collections::HashMap;
/// use dprint_core::configuration::{resolve_global_config};
/// use dprint_plugin_markdown::configuration::{resolve_config};
///
/// let config_map = HashMap::new(); // get a collection of key value pairs from somewhere
/// let global_config_result = resolve_global_config(&config_map);
///
/// // check global_config_result.diagnostics here...
///
/// let markdown_config_map = HashMap::new(); // get a collection of k/v pairs from somewhere
/// let config_result = resolve_config(
///     &markdown_config_map,
///     &global_config_result.config
/// );
///
/// // check config_result.diagnostics here and use config_result.config
/// ```
pub fn resolve_config(config: &HashMap<String, String>, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
    let mut diagnostics = Vec::new();
    let mut config = config.clone();

    let resolved_config = Configuration {
        line_width: get_value(&mut config, "lineWidth", global_config.line_width.unwrap_or(80), &mut diagnostics),
        use_tabs: get_value(&mut config, "useTabs", global_config.use_tabs.unwrap_or(DEFAULT_GLOBAL_CONFIGURATION.use_tabs), &mut diagnostics),
        indent_width: get_value(&mut config, "indentWidth", global_config.indent_width.unwrap_or(DEFAULT_GLOBAL_CONFIGURATION.indent_width), &mut diagnostics),
        new_line_kind: get_value(&mut config, "newLineKind", global_config.new_line_kind.unwrap_or(DEFAULT_GLOBAL_CONFIGURATION.new_line_kind), &mut diagnostics),
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

/// Resolved markdown configuration.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    pub indent_width: u8,
    pub line_width: u32,
    pub use_tabs: bool,
    pub new_line_kind: NewLineKind,
}

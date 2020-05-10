use std::collections::HashMap;
use dprint_core::configuration::{GlobalConfiguration, resolve_global_config, NewLineKind};

use super::{Configuration, resolve_config};

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
            resolve_config(self.config.clone(), global_config).config
        } else {
            let global_config = resolve_global_config(HashMap::new()).config;
            resolve_config(self.config.clone(), &global_config).config
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use dprint_core::configuration::{resolve_global_config, NewLineKind};

    use super::ConfigurationBuilder;
    use super::super::resolve_config;

    #[test]
    fn check_all_values_set() {
        let mut config = ConfigurationBuilder::new();
        config.new_line_kind(NewLineKind::CarriageReturnLineFeed)
            .line_width(90)
            .use_tabs(true)
            .indent_width(8);

        let inner_config = config.get_inner_config();
        assert_eq!(inner_config.len(), 4);
        let diagnostics = resolve_config(inner_config, &resolve_global_config(HashMap::new()).config).diagnostics;
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn handle_global_config() {
        let mut global_config = HashMap::new();
        global_config.insert(String::from("lineWidth"), String::from("90"));
        global_config.insert(String::from("indentWidth"), String::from("8"));
        global_config.insert(String::from("newLineKind"), String::from("crlf"));
        global_config.insert(String::from("useTabs"), String::from("true"));
        let global_config = resolve_global_config(global_config).config;
        let mut config_builder = ConfigurationBuilder::new();
        let config = config_builder.global_config(global_config).build();
        assert_eq!(config.line_width, 90);
        assert_eq!(config.indent_width, 8);
        assert_eq!(config.new_line_kind == NewLineKind::CarriageReturnLineFeed, true);
        assert_eq!(config.use_tabs, true);
    }

    #[test]
    fn use_markdown_defaults_when_global_not_set() {
        let global_config = resolve_global_config(HashMap::new()).config;
        let mut config_builder = ConfigurationBuilder::new();
        let config = config_builder.global_config(global_config).build();
        assert_eq!(config.line_width, 80); // this is different
        assert_eq!(config.indent_width, 4);
        assert_eq!(config.new_line_kind == NewLineKind::LineFeed, true);
        assert_eq!(config.use_tabs, false);
    }
}

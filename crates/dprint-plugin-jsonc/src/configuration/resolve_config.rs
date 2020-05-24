use dprint_core::configuration::*;
use std::collections::HashMap;
use super::Configuration;

/// Resolves configuration from a collection of key value strings.
///
/// # Example
///
/// ```
/// use std::collections::HashMap;
/// use dprint_core::configuration::{resolve_global_config};
/// use dprint_plugin_jsonc::configuration::{resolve_config};
///
/// let config_map = HashMap::new(); // get a collection of key value pairs from somewhere
/// let global_config_result = resolve_global_config(config_map);
///
/// // check global_config_result.diagnostics here...
///
/// let jsonc_config_map = HashMap::new(); // get a collection of k/v pairs from somewhere
/// let config_result = resolve_config(
///     jsonc_config_map,
///     &global_config_result.config
/// );
///
/// // check config_result.diagnostics here and use config_result.config
/// ```
pub fn resolve_config(config: HashMap<String, String>, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
    let mut diagnostics = Vec::new();
    let mut config = config;

    let resolved_config = Configuration {
        line_width: get_value(&mut config, "lineWidth", global_config.line_width.unwrap_or(DEFAULT_GLOBAL_CONFIGURATION.line_width), &mut diagnostics),
        use_tabs: get_value(&mut config, "useTabs", global_config.use_tabs.unwrap_or(DEFAULT_GLOBAL_CONFIGURATION.use_tabs), &mut diagnostics),
        indent_width: get_value(&mut config, "indentWidth", global_config.indent_width.unwrap_or(DEFAULT_GLOBAL_CONFIGURATION.indent_width), &mut diagnostics),
        new_line_kind: get_value(&mut config, "newLineKind", global_config.new_line_kind.unwrap_or(DEFAULT_GLOBAL_CONFIGURATION.new_line_kind), &mut diagnostics),
        comment_line_force_space_after_slashes: get_value(&mut config, "commentLine.forceSpaceAfterSlashes", true, &mut diagnostics),
    };

    diagnostics.extend(get_unknown_property_diagnostics(config));

    ResolveConfigurationResult {
        config: resolved_config,
        diagnostics,
    }
}

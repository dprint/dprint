use dprint_core::configuration::ConfigKeyMap;
use std::path::Path;
use dprint_core::types::ErrBox;

use crate::plugins::pool::PluginPools;
use crate::environment::Environment;

pub fn format_with_plugin_pool<TEnvironment: Environment>(
    parent_plugin_name: &str,
    file_path: &Path,
    file_text: &str,
    override_config: &ConfigKeyMap,
    pools: &PluginPools<TEnvironment>,
) -> Result<Option<String>, ErrBox> {
    if let Some(sub_plugin_name) = pools.get_plugin_name_from_file_name(file_path) {
        let initialized_plugin = pools.take_instance_for_plugin(&parent_plugin_name, &sub_plugin_name);
        match initialized_plugin {
            Ok(mut initialized_plugin) => {
                let format_result = initialized_plugin.format_text(&file_path, &file_text, &override_config);
                pools.release_instance_for_plugin(&parent_plugin_name, &sub_plugin_name, initialized_plugin);
                let formatted_text = format_result?; // do this after releasing
                Ok(if formatted_text == file_text {
                    None // no change
                } else {
                    Some(formatted_text)
                })
            },
            Err(err) => Err(err),
        }
    } else {
        Ok(None) // no plugin, no change
    }
}

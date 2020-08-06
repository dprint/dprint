use dprint_core::configuration::ConfigKeyMap;
use std::path::PathBuf;
use dprint_core::types::ErrBox;

use crate::plugins::pool::PluginPools;
use crate::environment::Environment;
use crate::utils::get_lowercase_file_extension;

pub fn format_with_plugin_pool<TEnvironment: Environment>(
    parent_plugin_name: &str,
    file_path: &PathBuf,
    file_text: &str,
    override_config: &ConfigKeyMap,
    pools: &PluginPools<TEnvironment>,
) -> Result<Option<String>, ErrBox> {
    let sub_plugin_name = if let Some(ext) = get_lowercase_file_extension(&file_path) {
        pools.get_plugin_name_from_extension(&ext)
    } else {
        None
    };

    if let Some(sub_plugin_name) = sub_plugin_name {
        let initialized_plugin = pools.take_instance_for_plugin(&parent_plugin_name, &sub_plugin_name);
        match initialized_plugin {
            Ok(initialized_plugin) => {
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

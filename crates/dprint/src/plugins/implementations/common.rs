use anyhow::Result;
use dprint_core::configuration::ConfigKeyMap;
use std::borrow::Cow;
use std::path::Path;

use crate::environment::Environment;
use crate::plugins::pool::PluginsCollection;

pub fn format_with_plugin_pool<TEnvironment: Environment>(
  parent_plugin_name: &str,
  file_path: &Path,
  file_text: &str,
  override_config: &ConfigKeyMap,
  pools: &PluginsCollection<TEnvironment>,
) -> Result<Option<String>> {
  let sub_plugin_names = pools.get_plugin_names_from_file_name(file_path);
  if sub_plugin_names.is_empty() {
    return Ok(None); // no plugin, no change
  }

  let initial_file_text = file_text;
  let mut file_text = Cow::Borrowed(file_text);
  for sub_plugin_name in sub_plugin_names {
    let initialized_plugin = pools.take_instance_for_plugin(parent_plugin_name, &sub_plugin_name);
    match initialized_plugin {
      Ok(mut initialized_plugin) => {
        let format_result = initialized_plugin.format_text(file_path, &file_text, override_config);
        pools.release_instance_for_plugin(parent_plugin_name, &sub_plugin_name, initialized_plugin);
        file_text = Cow::Owned(format_result?); // do this after releasing
      }
      Err(err) => return Err(err),
    }
  }

  if file_text == initial_file_text {
    Ok(None) // no change
  } else {
    Ok(Some(file_text.into_owned()))
  }
}

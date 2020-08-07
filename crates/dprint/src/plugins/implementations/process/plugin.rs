use std::sync::Arc;
use std::path::PathBuf;
use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration, ConfigKeyMap};
use dprint_core::plugins::PluginInfo;
use dprint_core::plugins::process::ProcessPluginCommunicator;
use dprint_core::types::ErrBox;

use crate::environment::Environment;
use crate::plugins::{Plugin, InitializedPlugin, PluginPools};

use super::super::format_with_plugin_pool;

static PLUGIN_FILE_INITIALIZE: std::sync::Once = std::sync::Once::new();

/// Use this to get an executable file name that also works in the tests.
pub fn get_test_safe_executable_path(executable_file_path: PathBuf, environment: &impl Environment) -> PathBuf {
    if environment.is_real() {
        executable_file_path
    } else {
        // do this so that we can launch the process in the tests
        if cfg!(target_os="windows") {
            let tmp_dir = PathBuf::from("temp");
            let temp_process_plugin_file = tmp_dir.join(if cfg!(target_os="windows") { "temp-plugin.exe" } else { "temp-plugin" });
            PLUGIN_FILE_INITIALIZE.call_once(|| {
                let _ = std::fs::create_dir(&tmp_dir);
                let _ = std::fs::write(&temp_process_plugin_file, environment.read_file_bytes(&executable_file_path).unwrap());
            });
            // ignore errors if path already exists
            temp_process_plugin_file
        } else {
            // couldn't figure out how to do chmod +x on a file in rust
            PathBuf::from("../../target/release/test-process-plugin")
        }
    }
}

pub struct ProcessPlugin<TEnvironment: Environment> {
    executable_file_path: PathBuf,
    plugin_info: PluginInfo,
    config: Option<(ConfigKeyMap, GlobalConfiguration)>,
    plugin_pools: Arc<PluginPools<TEnvironment>>,
}

impl<TEnvironment: Environment> ProcessPlugin<TEnvironment> {
    pub fn new(executable_file_path: PathBuf, plugin_info: PluginInfo, plugin_pools: Arc<PluginPools<TEnvironment>>) -> Self {
        ProcessPlugin {
            executable_file_path,
            plugin_info,
            config: None,
            plugin_pools
        }
    }
}

impl<TEnvironment: Environment> Plugin for ProcessPlugin<TEnvironment> {
    fn name(&self) -> &str {
        &self.plugin_info.name
    }

    fn version(&self) -> &str {
        &self.plugin_info.version
    }

    fn config_key(&self) -> &str {
        &self.plugin_info.config_key
    }

    fn file_extensions(&self) -> &Vec<String> {
        &self.plugin_info.file_extensions
    }

    fn help_url(&self) -> &str {
        &self.plugin_info.help_url
    }

    fn config_schema_url(&self) -> &str {
        &self.plugin_info.config_schema_url
    }

    fn set_config(&mut self, plugin_config: ConfigKeyMap, global_config: GlobalConfiguration) {
        self.config = Some((plugin_config, global_config));
    }

    fn get_config(&self) -> &(ConfigKeyMap, GlobalConfiguration) {
        self.config.as_ref().expect("Call set_config first.")
    }

    fn initialize(&self) -> Result<Box<dyn InitializedPlugin>, ErrBox> {
        let process_plugin = InitializedProcessPlugin::new(
            self.name().to_string(),
            &self.executable_file_path,
            Some(self.plugin_pools.clone())
        )?;
        let (plugin_config, global_config) = self.config.as_ref().expect("Call set_config first.");

        process_plugin.set_global_config(&global_config)?;
        process_plugin.set_plugin_config(&plugin_config)?;

        Ok(Box::new(process_plugin))
    }
}

pub struct InitializedProcessPlugin<TEnvironment: Environment> {
    name: String,
    communicator: ProcessPluginCommunicator,
    plugin_pools: Option<Arc<PluginPools<TEnvironment>>>,
}

impl<TEnvironment: Environment> InitializedProcessPlugin<TEnvironment> {
    pub fn new(
        name: String,
        executable_file_path: &PathBuf,
        plugin_pools: Option<Arc<PluginPools<TEnvironment>>>,
    ) -> Result<Self, ErrBox> {
        let initialized_plugin = InitializedProcessPlugin {
            name,
            plugin_pools,
            communicator: ProcessPluginCommunicator::new(executable_file_path)?,
        };

        Ok(initialized_plugin)
    }

    pub fn set_global_config(&self, global_config: &GlobalConfiguration) -> Result<(), ErrBox> {
        self.communicator.set_global_config(global_config)
    }

    pub fn set_plugin_config(&self, plugin_config: &ConfigKeyMap) -> Result<(), ErrBox> {
        self.communicator.set_plugin_config(plugin_config)
    }

    pub fn get_plugin_info(&self) -> Result<PluginInfo, ErrBox> {
        self.communicator.get_plugin_info()
    }
}

impl<TEnvironment: Environment> InitializedPlugin for InitializedProcessPlugin<TEnvironment> {
    fn get_license_text(&self) -> Result<String, ErrBox> {
        self.communicator.get_license_text()
    }

    fn get_resolved_config(&self) -> Result<String, ErrBox> {
        self.communicator.get_resolved_config()
    }

    fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>, ErrBox> {
        self.communicator.get_config_diagnostics()
    }

    fn format_text(&self, file_path: &PathBuf, file_text: &str, override_config: &ConfigKeyMap) -> Result<String, ErrBox> {
        self.communicator.format_text(file_path, file_text, override_config, |file_path, file_text, override_config| {
            let pools = self.plugin_pools.as_ref().unwrap();
            format_with_plugin_pool(&self.name, &file_path, &file_text, &override_config, pools)
        })
    }
}

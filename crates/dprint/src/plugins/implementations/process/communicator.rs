use std::cell::RefCell;
use std::path::PathBuf;
use dprint_core::configuration::{ConfigurationDiagnostic, GlobalConfiguration, ConfigKeyMap};
use dprint_core::plugins::process::ProcessPluginCommunicator;
use dprint_core::types::ErrBox;
use crate::environment::Environment;

/// A communicator that can recreate the process if it's unresponsive
/// and initializes the plugin with the configuration on each startup.
pub struct InitializedProcessPluginCommunicator<TEnvironment: Environment> {
    environment: TEnvironment,
    plugin_name: String,
    executable_file_path: PathBuf,
    config: (ConfigKeyMap, GlobalConfiguration),
    communicator: RefCell<ProcessPluginCommunicator>,
}

impl<TEnvironment: Environment> InitializedProcessPluginCommunicator<TEnvironment> {
    pub fn new(
        environment: TEnvironment,
        plugin_name: String,
        executable_file_path: PathBuf,
        config: (ConfigKeyMap, GlobalConfiguration),
    ) -> Result<Self, ErrBox> {
        let communicator = create_new_communicator(environment.clone(), plugin_name.clone(), &executable_file_path, &config)?;
        let initialized_communicator = InitializedProcessPluginCommunicator {
            environment,
            plugin_name,
            executable_file_path,
            config,
            communicator: RefCell::new(communicator),
        };

        Ok(initialized_communicator)
    }

    pub fn get_license_text(&self) -> Result<String, ErrBox> {
        self.communicator.borrow_mut().get_license_text()
    }

    pub fn get_resolved_config(&self) -> Result<String, ErrBox> {
        self.communicator.borrow_mut().get_resolved_config()
    }

    pub fn get_config_diagnostics(&self) -> Result<Vec<ConfigurationDiagnostic>, ErrBox> {
        self.communicator.borrow_mut().get_config_diagnostics()
    }

    pub fn recreate_process_if_dead(&self) -> Result<bool, ErrBox> {
        let is_process_alive = { self.communicator.borrow_mut().is_process_alive() };
        if is_process_alive {
            Ok(false)
        } else {
            self.force_recreate_process()?;
            Ok(true)
        }
    }

    pub fn force_recreate_process(&self) -> Result<(), ErrBox> {
        let new_communicator = create_new_communicator(
            self.environment.clone(),
            self.plugin_name.clone(),
            &self.executable_file_path,
            &self.config
        )?;
        let mut communicator = self.communicator.borrow_mut();
        *communicator = new_communicator;
        Ok(())
    }

    pub fn format_text(&self, file_path: &PathBuf, file_text: &str, override_config: &ConfigKeyMap, format_with_host: impl Fn(PathBuf, String, ConfigKeyMap) -> Result<Option<String>, ErrBox>) -> Result<String, ErrBox> {
        self.communicator.borrow_mut().format_text(file_path, file_text, override_config, format_with_host)
    }
}

fn create_new_communicator<TEnvironment: Environment>(
    environment: TEnvironment,
    plugin_name: String,
    executable_file_path: &PathBuf,
    config: &(ConfigKeyMap, GlobalConfiguration)
) -> Result<ProcessPluginCommunicator, ErrBox> {
    // ensure it's initialized each time
    let mut communicator = ProcessPluginCommunicator::new(executable_file_path, move |error_message| {
        environment.log_error_with_context(&error_message, &plugin_name);
    })?;
    communicator.set_global_config(&config.1)?;
    communicator.set_plugin_config(&config.0)?;
    Ok(communicator)
}

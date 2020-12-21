use dprint_cli_core::types::ErrBox;
use crate::environment::Environment;

pub fn handle_windows_install(environment: &impl Environment, install_path: &str) -> Result<(), ErrBox> {
    environment.ensure_system_path(install_path)
}

pub fn handle_windows_uninstall(environment: &impl Environment, install_path: &str) -> Result<(), ErrBox> {
    environment.remove_system_path(install_path)
}
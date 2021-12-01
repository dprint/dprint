use crate::environment::Environment;
use dprint_cli_core::types::ErrBox;

pub fn handle_windows_install(environment: &impl Environment, install_path: &str) -> Result<(), ErrBox> {
  environment.ensure_system_path(install_path)
}

pub fn handle_windows_uninstall(environment: &impl Environment, install_path: &str) -> Result<(), ErrBox> {
  environment.remove_system_path(install_path)
}

#[cfg(test)]
mod test {
  use std::path::PathBuf;

  use crate::environment::Environment;
  use crate::environment::TestEnvironment;
  use crate::test_helpers::run_test_cli;

  #[test]
  #[cfg(windows)]
  fn should_install_and_uninstall_on_windows() {
    let environment = TestEnvironment::new();
    environment.ensure_system_path("C:\\other").unwrap();
    run_test_cli(vec!["hidden", "windows-install", "C:\\test"], &environment).unwrap();
    assert_eq!(environment.get_system_path_dirs(), vec![PathBuf::from("C:\\other"), PathBuf::from("C:\\test")]);
    run_test_cli(vec!["hidden", "windows-uninstall", "C:\\test"], &environment).unwrap();
    assert_eq!(environment.get_system_path_dirs(), vec![PathBuf::from("C:\\other")]);
  }
}

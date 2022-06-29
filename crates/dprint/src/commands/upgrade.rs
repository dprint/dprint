use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;

use crate::environment::Environment;
use crate::environment::FilePermissions;
use crate::utils::extract_zip;
use crate::utils::latest_cli_version;

pub async fn upgrade<TEnvironment: Environment>(environment: &TEnvironment) -> Result<()> {
  let latest_version = latest_cli_version(environment).context("Error fetching latest CLI verison.")?;
  let current_version = environment.cli_version();
  if current_version == latest_version {
    environment.log(&format!("Already on latest version {}", latest_version));
    return Ok(());
  }

  environment.log(&format!("Upgrading from {} to {}...", current_version, latest_version));

  let exe_path = environment.current_exe()?;
  for component in exe_path.components() {
    if component.as_os_str().to_string_lossy().to_lowercase() == "node_modules" {
      bail!("Cannot upgrade with `dprint upgrade` when the dprint executable is within a node_modules folder. Upgrade with npm instead.");
    }
  }
  if exe_path.starts_with("/usr/local/Cellar/") {
    bail!("Cannot upgrade with `dprint upgrade` when the dprint executable is installed via Homebrew. Run `brew upgrade dprint` instead.");
  }

  let permissions = environment.file_permissions(&exe_path)?;

  if permissions.readonly() {
    bail!("You do not have write permission to {}", exe_path.display());
  }

  let arch = environment.cpu_arch();
  let os = environment.os();
  let zip_suffix = match os.as_str() {
    "linux" => "unknown-linux-gnu",
    "macos" => "apple-darwin",
    "windows" => "pc-windows-msvc",
    _ => bail!("Not implemented operating system: {}", os),
  };
  let zip_filename = format!("dprint-{}-{}.zip", arch, zip_suffix);
  let zip_url = format!("https://github.com/dprint/dprint/releases/download/{}/{}", latest_version, zip_filename);

  let zip_bytes = environment.download_file_err_404(&zip_url)?;
  let old_executable = exe_path.with_extension("old.exe");

  if !environment.is_real() {
    // kind of hard to test this with a test environment
    panic!("Need real environment.");
  }

  if cfg!(windows) {
    // on windows, we need to rename the current running executable
    // to something else in order to be able to replace it.
    environment.rename(&exe_path, &old_executable)?;
  }

  if let Err(err) = try_upgrade(&exe_path, &zip_bytes, permissions, environment) {
    if cfg!(windows) {
      // try to rename it back
      environment.rename(&old_executable, &exe_path).with_context(|| {
        format!(
          "Upgrade error: {:#}\nError upgrading and then error restoring. You may need to reinstall dprint from scratch. Sorry!",
          err
        )
      })?;
    }
    return Err(err);
  }

  // it would be nice if we could delete the old executable here on Windows,
  // but we need it in order to keep running the current executable

  Ok(())
}

fn try_upgrade(exe_path: &Path, zip_bytes: &[u8], permissions: FilePermissions, environment: &impl Environment) -> Result<()> {
  extract_zip("Extracting zip...", zip_bytes, exe_path.parent().unwrap(), environment)?;
  environment.set_file_permissions(exe_path, permissions)?;
  validate_executable(exe_path).context("Error validating new executable.")?;
  Ok(())
}

fn validate_executable(path: &Path) -> Result<()> {
  let status = Command::new(path).stderr(Stdio::null()).stdout(Stdio::null()).arg("-v").status()?;
  if !status.success() {
    bail!("Status was not success.");
  }
  Ok(())
}

#[cfg(test)]
mod test {
  use crate::environment::Environment;
  use crate::environment::FilePermissions;
  use crate::environment::TestEnvironment;
  use crate::environment::TestFilePermissions;
  use crate::test_helpers::run_test_cli;

  #[test]
  fn should_not_upgrade_same_version() {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.0.0" }"#.as_bytes());
    run_test_cli(vec!["upgrade"], &environment).unwrap();
    assert_eq!(environment.take_stdout_messages(), vec!["Already on latest version 0.0.0"]);
  }

  #[test]
  fn should_upgrade_and_fail_readonly() {
    let environment = TestEnvironment::new();
    environment
      .set_file_permissions(
        environment.current_exe().unwrap(),
        FilePermissions::Test(TestFilePermissions { readonly: true }),
      )
      .unwrap();
    environment.add_remote_file("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.1.0" }"#.as_bytes());
    let err = run_test_cli(vec!["upgrade"], &environment).err().unwrap();
    assert_eq!(
      err.to_string(),
      format!("You do not have write permission to {}", environment.current_exe().unwrap().display())
    );
    assert_eq!(environment.take_stdout_messages(), vec!["Upgrading from 0.0.0 to 0.1.0..."]);
  }

  #[test]
  fn should_upgrade_and_fail_node_modules() {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.1.0" }"#.as_bytes());
    environment.set_current_exe_path("/test/node_modules/dprint/dprint");
    let err = run_test_cli(vec!["upgrade"], &environment).err().unwrap();
    assert_eq!(
      err.to_string(),
      "Cannot upgrade with `dprint upgrade` when the dprint executable is within a node_modules folder. Upgrade with npm instead.",
    );
    assert_eq!(environment.take_stdout_messages(), vec!["Upgrading from 0.0.0 to 0.1.0..."]);
  }

  #[test]
  fn should_upgrade_and_fail_homebrew() {
    let environment = TestEnvironment::new();
    environment.add_remote_file("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.1.0" }"#.as_bytes());
    environment.set_current_exe_path("/usr/local/Cellar/dprint");
    let err = run_test_cli(vec!["upgrade"], &environment).err().unwrap();
    assert_eq!(
      err.to_string(),
      "Cannot upgrade with `dprint upgrade` when the dprint executable is installed via Homebrew. Run `brew upgrade dprint` instead.",
    );
    assert_eq!(environment.take_stdout_messages(), vec!["Upgrading from 0.0.0 to 0.1.0..."]);
  }

  #[test]
  fn should_upgrade_and_fail_different_version_no_remote_zip() {
    let environment = TestEnvironment::new();
    environment
      .set_file_permissions(environment.current_exe().unwrap(), FilePermissions::Test(Default::default()))
      .unwrap();
    environment.add_remote_file("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.1.0" }"#.as_bytes());
    let err = run_test_cli(vec!["upgrade"], &environment).err().unwrap();
    assert!(err.to_string().starts_with("Error downloading"));
    assert_eq!(environment.take_stdout_messages(), vec!["Upgrading from 0.0.0 to 0.1.0..."]);
  }
}

use crate::environment::Environment;
use anyhow::anyhow;
use anyhow::Result;
use serde_json::Value;

pub fn is_out_of_date(environment: &impl Environment) -> Option<String> {
  log_verbose!(environment, "Checking if CLI out of date...");
  match latest_cli_version(environment) {
    Ok(latest_version) => {
      let current_version = environment.cli_version();
      if current_version == latest_version {
        log_verbose!(environment, "CLI version matched.");
        None
      } else {
        log_verbose!(environment, "Current version: {}\nLatest version: {}", current_version, latest_version);
        Some(latest_version)
      }
    }
    Err(err) => {
      log_verbose!(environment, "Error fetching CLI version: {:#}", err);
      None
    }
  }
}

// todo: make async
pub fn latest_cli_version(environment: &impl Environment) -> Result<String> {
  let file_bytes = environment.download_file_err_404("https://plugins.dprint.dev/cli.json")?;
  let data: Value = serde_json::from_slice(&file_bytes)?;
  let obj = data.as_object().ok_or_else(|| anyhow!("Root was not object."))?;
  let version = obj.get("version").ok_or_else(|| anyhow!("Could not find version."))?;
  Ok(version.as_str().ok_or_else(|| anyhow!("version was not a string."))?.to_string())
}

#[cfg(test)]
mod test {
  use crate::environment::TestEnvironmentBuilder;

  use super::*;

  #[test]
  fn gets_latest_cli_version_valid() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_file("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.1.0" }"#)
      .build();
    assert_eq!(latest_cli_version(&environment).unwrap(), "0.1.0");
  }

  #[test]
  fn gets_latest_cli_version_if_out_of_date() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_file("https://plugins.dprint.dev/cli.json", r#"{ "version": "2.2.1" }"#)
      .build();
    assert_eq!(is_out_of_date(&environment), Some("2.2.1".to_string()));
  }

  #[test]
  fn gets_if_not_out_of_date() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_file("https://plugins.dprint.dev/cli.json", r#"{ "version": "0.0.0" }"#)
      .build();
    assert_eq!(is_out_of_date(&environment), None);
  }

  #[test]
  fn is_out_of_date_invalid() {
    let environment = TestEnvironmentBuilder::new()
      .add_remote_file("https://plugins.dprint.dev/cli.json", r#"{}"#)
      .build();
    assert_eq!(is_out_of_date(&environment), None);
  }

  #[test]
  fn is_out_of_date_err() {
    let environment = TestEnvironmentBuilder::new().build();
    environment.add_remote_file_error("https://plugins.dprint.dev/cli.json", r#"err"#);
    assert_eq!(is_out_of_date(&environment), None);
  }
}

use anyhow::Result;

#[cfg(windows)]
pub fn get_running_pids_by_name(searching_name: &str) -> Result<Vec<u32>> {
  use std::process::Command;
  use std::process::Stdio;

  use anyhow::bail;

  let filter = format!("IMAGENAME eq {}.exe", searching_name);
  let output = Command::new("tasklist")
    .args([
      // csv format
      "/FO",
      "CSV",
      // no header
      "/NH",
      // filter by process name
      "/FI",
      filter.as_str(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()?;
  if !output.status.success() {
    bail!("Error getting process names: {}", String::from_utf8(output.stderr)?);
  }
  let stdout = String::from_utf8(output.stdout)?;
  let lines = stdout.lines();

  Ok(
    lines
      .filter_map(|line| line.split(',').nth(1).and_then(|p| p.trim_matches('"').parse::<u32>().ok()))
      .collect(),
  )
}

#[cfg(not(windows))]
pub fn get_running_pids_by_name(searching_name: &str) -> Result<Vec<u32>> {
  use std::process::Command;
  use std::process::Stdio;

  use anyhow::bail;

  let output = Command::new("ps")
    .args([
      "-A", "-o", "pid=", // equals, for no header
      "-o", "comm=", // not cmd, because comm works on mac as well
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()?;
  if !output.status.success() {
    bail!("Error getting process names: {}", String::from_utf8(output.stderr)?);
  }
  let stdout = String::from_utf8(output.stdout)?;
  let lines = stdout.lines();
  Ok(
    lines
      .filter_map(|line| {
        let line = line.trim();
        let first_space = line.find(' ')?;
        let pid = &line[..first_space];
        let command_name = &line[first_space + 1..];
        let pid = pid.parse::<u32>().ok()?;
        if command_name == searching_name || command_name.ends_with(&format!("/{}", searching_name)) {
          Some(pid)
        } else {
          None
        }
      })
      .collect(),
  )
}

#[cfg(windows)]
pub fn kill_process_by_id(pid: u32) -> Result<()> {
  let pid_string = pid.to_string();
  run_command(vec!["taskkill", "/F", "/PID", pid_string.as_str()])
}

#[cfg(not(windows))]
pub fn kill_process_by_id(pid: u32) -> Result<()> {
  let pid_string = pid.to_string();
  run_command(vec!["kill", pid_string.as_str()])
}

fn run_command(mut command: Vec<&str>) -> Result<()> {
  use std::process::Command;
  use std::process::Stdio;

  use anyhow::bail;

  let output = Command::new(command.remove(0))
    .args(command)
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .output()?;
  if !output.status.success() {
    bail!("Error: {}", String::from_utf8(output.stderr)?);
  }

  Ok(())
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn gets_process_ids() {
    let results = get_running_pids_by_name("cargo").unwrap();
    assert!(!results.is_empty());
    let results = get_running_pids_by_name("dprint-testing-not-exists").unwrap();
    assert!(results.is_empty());
  }
}

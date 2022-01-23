use std::thread;
use std::time::Duration;

/// Starts a thread that polls for the existence of the parent process.
/// If the parent process no longer exists, then it will exit the current process.
pub fn start_parent_process_checker_thread(parent_process_id: u32) -> thread::JoinHandle<()> {
  thread::spawn(move || loop {
    thread::sleep(Duration::from_secs(30));

    if !is_process_active(parent_process_id) {
      std::process::exit(1);
    }
  })
}

/// Gets the parent process id from the CLI arguments.
///
/// The dprint cli will provide a `--parent-pid <value>` flag to specify the parent PID.
pub fn get_parent_process_id_from_cli_args() -> Option<u32> {
  let args: Vec<String> = std::env::args().collect();
  for i in 0..args.len() {
    if args[i] == "--parent-pid" {
      if let Some(parent_pid) = args.get(i + 1) {
        return parent_pid.parse::<u32>().map(Some).unwrap_or(None);
      }
    }
  }

  None
}

// code below is from my implementation when adding this to Deno

#[cfg(unix)]
fn is_process_active(process_id: u32) -> bool {
  unsafe {
    // signal of 0 checks for the existence of the process id
    libc::kill(process_id as i32, 0) == 0
  }
}

#[cfg(windows)]
fn is_process_active(process_id: u32) -> bool {
  use winapi::shared::minwindef::DWORD;
  use winapi::shared::minwindef::FALSE;
  use winapi::shared::ntdef::NULL;
  use winapi::shared::winerror::WAIT_TIMEOUT;
  use winapi::um::handleapi::CloseHandle;
  use winapi::um::processthreadsapi::OpenProcess;
  use winapi::um::synchapi::WaitForSingleObject;
  use winapi::um::winnt::SYNCHRONIZE;

  unsafe {
    let process = OpenProcess(SYNCHRONIZE, FALSE, process_id as DWORD);
    let result = if process == NULL {
      false
    } else {
      WaitForSingleObject(process, 0) == WAIT_TIMEOUT
    };

    CloseHandle(process);
    result
  }
}

#[cfg(test)]
mod test {
  use super::is_process_active;
  use std::path::PathBuf;
  use std::process::Command;

  #[test]
  fn should_tell_when_process_active() {
    if !get_dprint_exe().exists() {
      panic!("Please run `cargo build` before running the tests.")
    }
    // launch a long running process
    let mut child = Command::new(get_dprint_exe())
      .arg("editor-service")
      .arg("--parent-pid")
      .arg(std::process::id().to_string())
      .spawn()
      .unwrap();

    let pid = child.id();
    assert_eq!(is_process_active(pid), true);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(is_process_active(pid), false);
  }

  fn get_dprint_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR").to_string())
      .join("../../target")
      .join(if cfg!(debug_assertions) { "debug" } else { "release" })
      .join(if cfg!(target_os = "windows") { "dprint.exe" } else { "dprint" })
  }
}

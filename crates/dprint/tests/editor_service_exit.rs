// runs the built dprint binary as an editor service over pipes and ensures
// the process exits after the close message while the editor keeps stdin
// open. this can't be a unit test because it's about the process exiting.
#![allow(clippy::disallowed_methods)] // standalone test binary; no Environment available

use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

// the process exits within milliseconds, but be generous for a busy machine
const TIMEOUT: Duration = Duration::from_secs(30);
const SUCCESS_BYTES: [u8; 4] = [255, 255, 255, 255];
const SUCCESS_RESPONSE_KIND: u32 = 0;
const CLOSE_KIND: u32 = 2;

#[test]
fn exits_after_close() {
  let temp_dir = tempfile::tempdir().unwrap();
  let cwd = temp_dir.path().join("cwd");
  let cache_dir = temp_dir.path().join("cache");
  std::fs::create_dir_all(&cwd).unwrap();
  std::fs::create_dir_all(&cache_dir).unwrap();
  let mut child = Command::new(env!("CARGO_BIN_EXE_dprint"))
    .args(["editor-service", "--parent-pid", &std::process::id().to_string()])
    .current_dir(&cwd)
    .env("DPRINT_CACHE_DIR", &cache_dir)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .unwrap();
  // keep stdin open while waiting for the process to exit
  let mut stdin = child.stdin.take().unwrap();
  let message_id = 1u32;
  let mut message = Vec::new();
  for value in [message_id, CLOSE_KIND, 0] {
    message.extend(value.to_be_bytes());
  }
  message.extend(SUCCESS_BYTES);
  stdin.write_all(&message).unwrap();
  stdin.flush().unwrap();

  let deadline = Instant::now() + TIMEOUT;
  let status = loop {
    if let Some(status) = child.try_wait().unwrap() {
      break status;
    }
    if Instant::now() >= deadline {
      let _ = child.kill();
      panic!("the process was still running {}s after the close message", TIMEOUT.as_secs());
    }
    std::thread::sleep(Duration::from_millis(10));
  };
  assert_eq!(status.code(), Some(0));

  // the success response was written before exiting
  let mut stdout = Vec::new();
  child.stdout.take().unwrap().read_to_end(&mut stdout).unwrap();
  let mut expected = Vec::new();
  // the first message the editor service sends has id 0
  for value in [0, SUCCESS_RESPONSE_KIND, 4, message_id] {
    expected.extend(value.to_be_bytes());
  }
  expected.extend(SUCCESS_BYTES);
  assert_eq!(stdout, expected);
  drop(stdin);
}

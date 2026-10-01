// runs the built dprint binary as a language server over pipes and ensures
// the process ends with the right exit code after the exit notification
// while the client keeps stdin open. this can't be a unit test because it's
// about the process exiting.
#![allow(clippy::disallowed_methods)] // standalone test binary; no Environment available

use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::process::Child;
use std::process::ChildStdin;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;

// the process exits within milliseconds, but be generous for a busy machine
const TIMEOUT: Duration = Duration::from_secs(30);

#[test]
fn exits_with_zero_after_shutdown_and_exit() {
  let mut server = LspProcess::spawn();
  server.send(json!({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    // no process id so that the parent process checker isn't started
    "params": { "processId": null, "capabilities": {} },
  }));
  server.read_response(1);
  server.send(json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" }));
  server.read_response(2);
  server.send(json!({ "jsonrpc": "2.0", "method": "exit" }));

  assert_eq!(server.wait_for_exit_with_stdin_open(), 0);
}

#[test]
fn exits_with_one_after_exit_without_shutdown() {
  let mut server = LspProcess::spawn();
  server.send(json!({ "jsonrpc": "2.0", "method": "exit" }));

  assert_eq!(server.wait_for_exit_with_stdin_open(), 1);
}

struct LspProcess {
  child: Child,
  stdin: ChildStdin,
  messages: mpsc::Receiver<Value>,
  // keeps the directories around until the process is done
  _temp_dir: tempfile::TempDir,
}

impl LspProcess {
  fn spawn() -> Self {
    let temp_dir = tempfile::tempdir().unwrap();
    let cwd = temp_dir.path().join("cwd");
    let cache_dir = temp_dir.path().join("cache");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&cache_dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dprint"))
      .arg("lsp")
      .current_dir(&cwd)
      .env("DPRINT_CACHE_DIR", &cache_dir)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .spawn()
      .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, messages) = mpsc::channel();
    std::thread::spawn(move || {
      let mut reader = BufReader::new(stdout);
      while let Some(message) = read_message(&mut reader) {
        if tx.send(message).is_err() {
          break;
        }
      }
    });
    Self {
      child,
      stdin,
      messages,
      _temp_dir: temp_dir,
    }
  }

  fn send(&mut self, message: Value) {
    let body = message.to_string();
    write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    self.stdin.flush().unwrap();
  }

  /// Waits for the response to the request with the provided id.
  fn read_response(&mut self, id: u64) -> Value {
    let deadline = Instant::now() + TIMEOUT;
    loop {
      let remaining = deadline.saturating_duration_since(Instant::now());
      let message = self
        .messages
        .recv_timeout(remaining)
        .unwrap_or_else(|err| panic!("failed receiving the response for request {}: {}", id, err));
      // skip over anything else the server sends (ex. log messages)
      if message.get("method").is_none() && message["id"] == id {
        assert!(message.get("error").is_none(), "request {} failed: {}", id, message);
        return message;
      }
    }
  }

  /// Waits for the process to exit without closing stdin, returning the exit code.
  fn wait_for_exit_with_stdin_open(&mut self) -> i32 {
    let deadline = Instant::now() + TIMEOUT;
    loop {
      if let Some(status) = self.child.try_wait().unwrap() {
        return status.code().expect("the process should have an exit code");
      }
      if Instant::now() >= deadline {
        panic!("the process was still running {}s after the exit notification", TIMEOUT.as_secs());
      }
      std::thread::sleep(Duration::from_millis(10));
    }
  }
}

impl Drop for LspProcess {
  fn drop(&mut self) {
    // don't leave the process running when a test fails
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

fn read_message(reader: &mut impl BufRead) -> Option<Value> {
  let mut content_length = None;
  loop {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
      return None; // stdout was closed
    }
    let line = line.trim_end();
    if line.is_empty() {
      break;
    }
    if let Some(value) = line.strip_prefix("Content-Length:") {
      content_length = Some(value.trim().parse::<usize>().ok()?);
    }
  }
  let mut body = vec![0; content_length?];
  reader.read_exact(&mut body).ok()?;
  serde_json::from_slice(&body).ok()
}

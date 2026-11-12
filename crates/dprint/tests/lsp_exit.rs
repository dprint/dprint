// runs the built dprint binary as a language server over pipes and ensures
// the process ends with the right exit code after the exit notification
// while the client keeps stdin open. this can't be a unit test because it's
// about the process exiting.
#![allow(clippy::disallowed_methods)] // standalone test binary; no Environment available

use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
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

#[test]
fn clear_cache_kills_the_language_servers_using_the_cache_dir() {
  let mut server = LspProcess::spawn();
  let cache_dir = server.cache_dir.clone();
  // a flag with a value before the subcommand
  let mut server_with_flag = LspProcess::spawn_with(&["--log-level", "debug", "lsp"], Some(&cache_dir));
  let mut other_server = LspProcess::spawn();
  for server in [&mut server, &mut server_with_flag, &mut other_server] {
    server.send(json!({
      "jsonrpc": "2.0",
      "id": 1,
      "method": "initialize",
      "params": { "processId": null, "capabilities": {} },
    }));
    server.read_response(1);
  }

  let status = Command::new(env!("CARGO_BIN_EXE_dprint"))
    .arg("clear-cache")
    .env("DPRINT_CACHE_DIR", &cache_dir)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .unwrap();
  assert!(status.success());

  server.wait_for_exit_with_stdin_open();
  server_with_flag.wait_for_exit_with_stdin_open();
  // the one that uses another cache directory keeps running
  assert!(other_server.child.try_wait().unwrap().is_none());
}

// the unit tests call the methods of the server, so this is what goes through
// the process and its pipes like a client does
#[test]
fn formats_a_document_for_a_client_without_capabilities() {
  let mut server = LspProcess::spawn();
  let plugin_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-plugin/test_plugin.wasm");
  std::fs::copy(plugin_path, server.cwd.join("plugin.wasm")).unwrap();
  std::fs::write(server.cwd.join("dprint.json"), r#"{ "plugins": ["./plugin.wasm"] }"#).unwrap();
  let file_uri = url::Url::from_file_path(server.cwd.join("file.txt")).unwrap();

  server.send(json!({
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": { "processId": null, "capabilities": {} },
  }));
  let capabilities = server.read_response(1)["result"]["capabilities"].clone();
  assert_eq!(capabilities["documentFormattingProvider"], json!(true));
  // only an editor extension is told about the dprint methods
  assert!(capabilities.get("experimental").is_none());
  server.send(json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
  server.send(json!({
    "jsonrpc": "2.0",
    "method": "textDocument/didOpen",
    "params": { "textDocument": { "uri": file_uri, "languageId": "plaintext", "version": 1, "text": "text" } },
  }));
  server.send(json!({
    "jsonrpc": "2.0",
    "id": 2,
    "method": "textDocument/formatting",
    "params": { "textDocument": { "uri": file_uri }, "options": { "tabSize": 2, "insertSpaces": true } },
  }));
  assert_eq!(
    server.read_response(2)["result"],
    json!([{
      "range": { "start": { "line": 0, "character": 4 }, "end": { "line": 0, "character": 4 } },
      "newText": "_formatted",
    }])
  );

  server.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "shutdown" }));
  server.read_response(3);
  server.send(json!({ "jsonrpc": "2.0", "method": "exit" }));
  assert_eq!(server.wait_for_exit_with_stdin_open(), 0);
}

struct LspProcess {
  child: Child,
  stdin: ChildStdin,
  messages: mpsc::Receiver<Value>,
  cwd: PathBuf,
  cache_dir: PathBuf,
  // keeps the directories around until the process is done
  _temp_dir: tempfile::TempDir,
}

impl LspProcess {
  fn spawn() -> Self {
    Self::spawn_with(&["lsp"], None)
  }

  /// Spawns dprint with the arguments, which uses its own cache directory
  /// unless one is provided.
  fn spawn_with(args: &[&str], cache_dir: Option<&Path>) -> Self {
    let temp_dir = tempfile::tempdir().unwrap();
    let cwd = temp_dir.path().join("cwd");
    let cache_dir = cache_dir.map(|dir| dir.to_path_buf()).unwrap_or_else(|| temp_dir.path().join("cache"));
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&cache_dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dprint"))
      .args(args)
      .current_dir(&cwd)
      .env("DPRINT_CACHE_DIR", &cache_dir)
      // not the settings and approvals of the user that runs the tests
      .env("DPRINT_CONFIG_DIR", temp_dir.path().join("config"))
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
      cwd,
      cache_dir,
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
        // a process that was killed doesn't have one on Unix
        return status.code().unwrap_or(-1);
      }
      if Instant::now() >= deadline {
        panic!("the process was still running after {}s", TIMEOUT.as_secs());
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

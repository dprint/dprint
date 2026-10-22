// runs the built dprint binary and ensures that everything a successful command
// has to do is finished before the process exits. `main` exits once `run()`
// returns instead of unwinding (which frees every loaded Wasm module, and on
// macOS that is slow), so nothing may be left for a destructor to do. this
// can't be a unit test because it's about the process exiting.
#![allow(clippy::disallowed_methods)] // standalone test binary; no Environment available

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

/// The plugin the in-process tests use. It handles `.txt` files and its
/// `ending` option defaults to "formatted", so formatting `text` produces
/// `text_formatted`, an observable result with no network access.
const TEST_PLUGIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../test-plugin/test_plugin.wasm");

#[test]
fn fmt_writes_the_file_and_the_incremental_cache_before_exiting() {
  let run = Run::new();
  let file = run.write_file("a.txt", "text");

  let output = run.dprint(&["fmt", "a.txt"], None);
  assert_eq!(output.status.code(), Some(0), "fmt failed: {}", String::from_utf8_lossy(&output.stderr));

  assert_eq!(
    std::fs::read_to_string(&file).unwrap(),
    "text_formatted",
    "the formatted text must be on disk after the process exits"
  );
  assert!(
    run.incremental_cache_file_count() > 0,
    "the incremental cache must be saved before the process exits"
  );
}

#[test]
fn stdin_output_is_complete_when_the_process_exits() {
  let run = Run::new();

  // the plugin's output has no trailing newline, so a buffered line writer
  // would keep all of it until it's flushed
  let output = run.dprint(&["fmt", "--stdin", "a.txt"], Some("text"));
  assert_eq!(output.status.code(), Some(0), "fmt failed: {}", String::from_utf8_lossy(&output.stderr));
  assert_eq!(String::from_utf8_lossy(&output.stdout), "text_formatted");
}

#[test]
fn exits_with_zero_when_there_is_nothing_to_format() {
  let run = Run::new();
  run.write_file("b.txt", "text_formatted");

  let output = run.dprint(&["fmt", "b.txt"], None);
  assert_eq!(output.status.code(), Some(0), "fmt failed: {}", String::from_utf8_lossy(&output.stderr));
}

struct Run {
  temp_dir: tempfile::TempDir,
  cwd: PathBuf,
}

impl Run {
  fn new() -> Self {
    let temp_dir = tempfile::tempdir().unwrap();
    let cwd = temp_dir.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(temp_dir.path().join("cache")).unwrap();
    let config = format!(r#"{{ "plugins": ["{}"] }}"#, TEST_PLUGIN.replace('\\', "/"));
    std::fs::write(cwd.join("dprint.json"), config).unwrap();
    Run { temp_dir, cwd }
  }

  fn write_file(&self, name: &str, text: &str) -> PathBuf {
    let path = self.cwd.join(name);
    std::fs::write(&path, text).unwrap();
    path
  }

  fn dprint(&self, args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dprint"))
      .args(args)
      .current_dir(&self.cwd)
      .env("DPRINT_CACHE_DIR", self.temp_dir.path().join("cache"))
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .unwrap();
    // dropping stdin closes it so the process doesn't wait on it
    let mut child_stdin = child.stdin.take().unwrap();
    if let Some(stdin) = stdin {
      child_stdin.write_all(stdin.as_bytes()).unwrap();
    }
    drop(child_stdin);
    child.wait_with_output().unwrap()
  }

  /// Counts the files the incremental cache keeps in the cache directory.
  fn incremental_cache_file_count(&self) -> usize {
    fn walk(dir: &Path, found: &mut usize) {
      let Ok(entries) = std::fs::read_dir(dir) else {
        return;
      };
      for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
          walk(&path, found);
        } else if path.components().any(|component| component.as_os_str() == "incremental") {
          *found += 1;
        }
      }
    }
    let mut found = 0;
    walk(&self.temp_dir.path().join("cache"), &mut found);
    found
  }
}

use std::io;
use std::io::BufRead;
use std::io::Read;
use std::path::Path;

use sys_traits::FsOpen;
use sys_traits::OpenOptions;

use crate::environment::Environment;

/// Whether the file's first line is a shebang matching one of the configured shebangs.
pub fn file_has_matching_shebang(environment: &impl Environment, file_path: &Path, shebangs: &[String]) -> bool {
  let Ok(Some(shebang_line)) = read_file_shebang_line(environment, file_path) else {
    return false;
  };
  let Some(shebang_line) = get_shebang_line(&shebang_line) else {
    return false;
  };
  shebangs.iter().any(|shebang| is_shebang_prefix_match(shebang_line, shebang))
}

/// Reads the first line of a file when it starts with a shebang (`#!`),
/// otherwise returns `None`. Only reads up to the end of the first line, so
/// large files that aren't scripts don't get read into memory.
pub fn read_file_shebang_line(environment: &impl Environment, file_path: &Path) -> io::Result<Option<Vec<u8>>> {
  log_debug!(environment, "Reading shebang line: {}", file_path.display());
  let map_err = |err: io::Error| io::Error::new(err.kind(), format!("Error reading file {}: {:#}", file_path.display(), err));
  let file = environment.fs_open(file_path, &OpenOptions::new_read()).map_err(map_err)?;
  let mut reader = io::BufReader::with_capacity(512, file);
  let mut start = [0u8; 2];
  match reader.read_exact(&mut start) {
    Ok(()) => {}
    // the file is shorter than a shebang
    Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
    Err(err) => return Err(map_err(err)),
  }
  if start != *b"#!" {
    return Ok(None);
  }
  // Bail when the line is unreasonably long since it can't be a shebang
  // (ex. a binary file that happens to start with `#!`). This is well above
  // the kernel's shebang line limit.
  const MAX_SHEBANG_LINE_LEN: usize = 4096;
  let mut line = start.to_vec();
  reader
    .take((MAX_SHEBANG_LINE_LEN - line.len()) as u64)
    .read_until(b'\n', &mut line)
    .map_err(map_err)?;
  let found_newline = line.last() == Some(&b'\n');
  if !found_newline && line.len() >= MAX_SHEBANG_LINE_LEN {
    return Ok(None);
  }
  Ok(Some(line))
}

/// Gets the trimmed first line of the file when it starts with a shebang (`#!`).
pub fn get_shebang_line(file_bytes_start: &[u8]) -> Option<&str> {
  if !file_bytes_start.starts_with(b"#!") {
    return None;
  }
  let end = file_bytes_start.iter().position(|b| *b == b'\n').unwrap_or(file_bytes_start.len());
  std::str::from_utf8(&file_bytes_start[..end]).ok().map(|line| line.trim())
}

/// Whether the shebang line equals the configured shebang or starts with it
/// followed by whitespace.
pub fn is_shebang_prefix_match(shebang_line: &str, configured_shebang: &str) -> bool {
  match shebang_line.strip_prefix(configured_shebang) {
    Some(rest) => rest.is_empty() || rest.starts_with(char::is_whitespace),
    None => false,
  }
}

#[cfg(test)]
mod test {
  use std::path::PathBuf;

  use crate::environment::TestEnvironment;

  use super::*;

  #[test]
  fn read_shebang_line() {
    fn read(text: &[u8]) -> Option<Vec<u8>> {
      let environment = TestEnvironment::new();
      let path = PathBuf::from("/file");
      environment.write_file_bytes(&path, text).unwrap();
      read_file_shebang_line(&environment, &path).unwrap()
    }

    assert_eq!(read(b""), None);
    assert_eq!(read(b"#"), None);
    assert_eq!(read(b"#!"), Some(b"#!".to_vec()));
    assert_eq!(read(b"# comment\n#!/bin/sh\n"), None);
    assert_eq!(read(b"#!/bin/sh"), Some(b"#!/bin/sh".to_vec()));
    assert_eq!(read(b"#!/bin/sh\ntext\n"), Some(b"#!/bin/sh\n".to_vec()));
    assert_eq!(read(b"#!/bin/sh\r\ntext\r\n"), Some(b"#!/bin/sh\r\n".to_vec()));
    assert_eq!(read(b"\xEF\xBB\xBF#!/bin/sh\n"), None); // bom
    // a line that's too long
    assert_eq!(read(format!("#!{}\ntext", "a".repeat(4093)).as_bytes()).map(|l| l.len()), Some(4096));
    assert_eq!(read(format!("#!{}", "a".repeat(4094)).as_bytes()), None);
    assert_eq!(read(format!("#!{}\ntext", "a".repeat(4094)).as_bytes()), None);
    // missing file
    let environment = TestEnvironment::new();
    assert!(read_file_shebang_line(&environment, &PathBuf::from("/missing")).is_err());
  }

  #[test]
  fn shebang_line() {
    assert_eq!(get_shebang_line(b"#!/bin/sh\ntext"), Some("#!/bin/sh"));
    assert_eq!(get_shebang_line(b"#!/bin/sh \r\ntext"), Some("#!/bin/sh"));
    assert_eq!(get_shebang_line(b"#!/bin/sh"), Some("#!/bin/sh"));
    assert_eq!(get_shebang_line(b"text"), None);
    assert_eq!(get_shebang_line(b""), None);
    // non-utf8
    assert_eq!(get_shebang_line(b"#!/bin/sh\xff\n"), None);
  }

  #[test]
  fn shebang_prefix_match() {
    assert!(is_shebang_prefix_match("#!/usr/bin/env deno run", "#!/usr/bin/env deno run"));
    assert!(is_shebang_prefix_match("#!/usr/bin/env deno run --allow-read", "#!/usr/bin/env deno run"));
    assert!(is_shebang_prefix_match("#!/usr/bin/env deno run --allow-read", "#!/usr/bin/env deno"));
    assert!(is_shebang_prefix_match("#!/usr/bin/env deno\trun", "#!/usr/bin/env deno"));
    assert!(!is_shebang_prefix_match("#!/usr/bin/env deno runtest", "#!/usr/bin/env deno run"));
    assert!(!is_shebang_prefix_match("#!/usr/bin/env nodemon", "#!/usr/bin/env node"));
    assert!(!is_shebang_prefix_match("#!/bin/shell", "#!/bin/sh"));
    assert!(!is_shebang_prefix_match("#!/bin/sh", "#!/bin/sh -e"));
    assert!(!is_shebang_prefix_match("#!/usr/bin/env -S deno run", "#!/usr/bin/env deno run"));
  }
}

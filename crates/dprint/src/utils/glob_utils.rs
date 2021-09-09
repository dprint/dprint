use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dprint_cli_core::types::ErrBox;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use parking_lot::{Condvar, Mutex};

use crate::environment::{DirEntry, DirEntryKind, Environment};

pub fn glob(environment: &impl Environment, base: impl AsRef<Path>, file_patterns: &Vec<String>) -> Result<Vec<PathBuf>, ErrBox> {
  if file_patterns.iter().all(|p| is_negated_glob(p)) {
    // performance improvement (see issue #379)
    log_verbose!(environment, "Skipping negated globs: {:?}", file_patterns);
    return Ok(Vec::with_capacity(0));
  }

  let start_instant = std::time::Instant::now();
  log_verbose!(environment, "Globbing: {:?}", file_patterns);

  let glob_matcher = GlobMatcher::new(
    file_patterns,
    &GlobMatcherOptions {
      case_insensitive: cfg!(windows),
    },
  )?;

  println!("STARTING...");
  let start_dir = base.as_ref().to_path_buf();
  let shared_state = Arc::new(SharedState::new(start_dir));

  // run the `fs::read_dir` calls on one thread
  let read_dir_runner = ReadDirRunner::new(environment.clone(), shared_state.clone());
  rayon::spawn(move || read_dir_runner.run());

  // run the glob matching on the current thread (the two threads will communicate with each other)
  let glob_matching_processor = GlobMatchingProcessor::new(shared_state, glob_matcher);
  let results = glob_matching_processor.run()?;

  log_verbose!(environment, "File(s) matched: {:?}", results);
  log_verbose!(environment, "Finished globbing in {}ms", start_instant.elapsed().as_millis());

  Ok(results)
}

const PUSH_DIR_ENTRIES_BATCH_COUNT: usize = 500;

struct ReadDirRunner<TEnvironment: Environment> {
  environment: TEnvironment,
  shared_state: Arc<SharedState>,
}

impl<TEnvironment: Environment> ReadDirRunner<TEnvironment> {
  pub fn new(environment: TEnvironment, shared_state: Arc<SharedState>) -> Self {
    Self { environment, shared_state }
  }

  pub fn run(&self) {
    let mut read_dir_time = 0;
    let mut push_entries_time = 0;
    loop {
      match self.get_next_pending_dirs() {
        Some(pending_dirs) => {
          let mut all_entries = Vec::new();
          for current_dir in pending_dirs.into_iter().flatten() {
            let elapsed = std::time::Instant::now();
            let info_result = self
              .environment
              .dir_info(&current_dir)
              .map_err(|err| err_obj!("error reading dir {}: {}", current_dir.display(), err.to_string()));
            match info_result {
              Ok(entries) => {
                read_dir_time += elapsed.elapsed().as_nanos();
                if !entries.is_empty() {
                  all_entries.extend(entries);
                  // it is much faster to batch these than to hit the lock every time
                  if all_entries.len() > PUSH_DIR_ENTRIES_BATCH_COUNT {
                    let elapsed = std::time::Instant::now();
                    self.push_entries(std::mem::take(&mut all_entries));
                    push_entries_time += elapsed.elapsed().as_nanos();
                  }
                }
              }
              Err(err) => {
                self.set_glob_error(err);
                return;
              }
            }
          }
          if !all_entries.is_empty() {
            let elapsed = std::time::Instant::now();
            self.push_entries(all_entries);
            push_entries_time += elapsed.elapsed().as_nanos();
          }
        }
        None => break,
      }
    }

    println!("READ DIR TIME: {}ms", read_dir_time / 1000000);
    println!("PUSH ENTRIES TIME: {}ms", push_entries_time / 1000000);
  }

  fn get_next_pending_dirs(&self) -> Option<Vec<Vec<PathBuf>>> {
    let &(ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    loop {
      if !state.pending_dirs.is_empty() {
        state.read_dir_thread_state = ReadDirThreadState::Processing;
        cvar.notify_one();
        return Some(std::mem::take(&mut state.pending_dirs));
      }
      if state.matching_thread_complete {
        state.read_dir_thread_state = ReadDirThreadState::Complete;
        cvar.notify_one();
        return None;
      } else {
        state.read_dir_thread_state = ReadDirThreadState::Waiting;
        cvar.notify_one();
        // wait to be notified by the other thread
        cvar.wait(&mut state);
      }
    }
  }

  fn set_glob_error(&self, error: ErrBox) {
    let &(ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.read_dir_thread_state = ReadDirThreadState::Error(error);
    cvar.notify_one();
  }

  fn push_entries(&self, entries: Vec<DirEntry>) {
    let &(ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.pending_entries.push(entries);
    cvar.notify_one();
  }
}

struct GlobMatchingProcessor {
  shared_state: Arc<SharedState>,
  glob_matcher: GlobMatcher,
}

impl GlobMatchingProcessor {
  pub fn new(shared_state: Arc<SharedState>, glob_matcher: GlobMatcher) -> Self {
    Self { shared_state, glob_matcher }
  }
  pub fn run(&self) -> Result<Vec<PathBuf>, ErrBox> {
    let mut results = Vec::new();
    let mut block_time = 0;
    let mut match_time = 0;

    loop {
      let elapsed = std::time::Instant::now();
      let mut pending_dirs = Vec::new();

      match self.get_next_entries() {
        Ok(None) => {
          println!("BLOCK TIME: {}ms", block_time / 1000000);
          println!("MATCH TIME: {}ms", match_time / 1000000);
          println!("RESULT COUNT: {}", results.len());

          return Ok(results);
        }
        Err(err) => return Err(err), // error
        Ok(Some(entries)) => {
          block_time += elapsed.elapsed().as_nanos();
          let elapsed = std::time::Instant::now();
          for entry in entries.into_iter().flatten() {
            match entry.kind {
              DirEntryKind::Directory => {
                if !self.glob_matcher.is_ignored(&entry.path) {
                  pending_dirs.push(entry.path);
                }
              }
              DirEntryKind::File => {
                if self.glob_matcher.is_match(&entry.path) {
                  results.push(entry.path);
                }
              }
            }
          }
          match_time += elapsed.elapsed().as_nanos();
        }
      }

      self.push_pending_dirs(pending_dirs);
    }
  }

  fn push_pending_dirs(&self, pending_dirs: Vec<PathBuf>) {
    let &(ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.pending_dirs.push(pending_dirs);
    cvar.notify_one();
  }

  fn get_next_entries(&self) -> Result<Option<Vec<Vec<DirEntry>>>, ErrBox> {
    let &(ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    loop {
      state.matching_thread_complete = false;
      if !state.pending_entries.is_empty() {
        return Ok(Some(std::mem::take(&mut state.pending_entries)));
      }
      match &state.read_dir_thread_state {
        ReadDirThreadState::Waiting => {
          state.matching_thread_complete = true;
          // notify the other thread that we're done too
          cvar.notify_one();
          // wait to be notified by the other thread
          cvar.wait(&mut state);
        }
        ReadDirThreadState::Complete => {
          return Ok(None);
        }
        ReadDirThreadState::Error(err) => {
          return Err(err_obj!("{}", err.to_string()));
        }
        ReadDirThreadState::Processing => {
          // wait to be notified by the other thread
          cvar.wait(&mut state);
        }
      }
    }
  }
}

enum ReadDirThreadState {
  Processing,
  Waiting,
  Complete,
  Error(ErrBox),
}

struct SharedStateInternal {
  pending_dirs: Vec<Vec<PathBuf>>,
  pending_entries: Vec<Vec<DirEntry>>,
  read_dir_thread_state: ReadDirThreadState,
  matching_thread_complete: bool,
}

struct SharedState {
  inner: (Mutex<SharedStateInternal>, Condvar),
}

impl SharedState {
  pub fn new(initial_dir: PathBuf) -> Self {
    SharedState {
      inner: (
        Mutex::new(SharedStateInternal {
          matching_thread_complete: false,
          read_dir_thread_state: ReadDirThreadState::Processing,
          pending_dirs: vec![vec![initial_dir]],
          pending_entries: Vec::new(),
        }),
        Condvar::new(),
      ),
    }
  }
}

pub fn to_absolute_globs(file_patterns: Vec<String>, base_dir: &str) -> Vec<String> {
  file_patterns.into_iter().map(|p| to_absolute_glob(&p, base_dir)).collect()
}

pub fn to_absolute_glob(pattern: &str, dir: &str) -> String {
  // Adapted from https://github.com/dsherret/ts-morph/blob/0f8a77a9fa9d74e32f88f36992d527a2f059c6ac/packages/common/src/fileSystem/FileUtils.ts#L272

  // convert backslashes to forward slashes (don't worry about matching file names with back slashes)
  let mut pattern = pattern.replace("\\", "/");
  let dir = dir.replace("\\", "/");

  // check to see if glob is negated
  let is_negated = is_negated_glob(&pattern);
  if is_negated {
    pattern.drain(..1); // remove the leading "!"
  }

  // .gitignore: "If there is a separator at the beginning or middle (or both) of
  // the pattern, then the pattern is relative to the directory level of the particular
  // .gitignore file itself. Otherwise the pattern may also match at any level below the
  // .gitignore level."
  let is_relative = match pattern.find("/") {
    Some(index) => index != pattern.len() - 1, // not the end of the pattern
    None => false,
  };

  // trim starting ./ from glob patterns
  if pattern.starts_with("./") {
    pattern.drain(..2);
  }

  // when the glob pattern is only a . use an empty string
  if pattern == "." {
    pattern = String::new();
  }

  // store last character before glob is modified
  let suffix = pattern.chars().last();

  // make glob absolute
  if !is_absolute_pattern(&pattern) {
    if is_relative || pattern.starts_with("**/") || pattern.trim().is_empty() {
      pattern = glob_join(dir, pattern);
    } else {
      pattern = glob_join(dir, format!("**/{}", pattern));
    }
  }

  // if glob had a trailing `/`, re-add it back
  if suffix == Some('/') && !pattern.ends_with('/') {
    pattern.push('/');
  }

  if is_negated {
    format!("!{}", pattern)
  } else {
    pattern
  }
}

pub fn is_negated_glob(pattern: &str) -> bool {
  let mut chars = pattern.chars();
  let first_char = chars.next();
  let second_char = chars.next();

  return first_char == Some('!') && second_char != Some('(');
}

fn glob_join(dir: String, pattern: String) -> String {
  // strip trailing slash
  let dir = if dir.ends_with('/') {
    Cow::Borrowed(&dir[..dir.len() - 1])
  } else {
    Cow::Owned(dir)
  };
  // strip leading slash
  let pattern = if pattern.starts_with('/') {
    Cow::Borrowed(&pattern[1..])
  } else {
    Cow::Owned(pattern)
  };

  if pattern.len() == 0 {
    dir.into_owned()
  } else {
    format!("{}/{}", dir, pattern)
  }
}

pub fn is_absolute_pattern(pattern: &str) -> bool {
  let pattern = if is_negated_glob(pattern) { &pattern[1..] } else { &pattern };
  pattern.starts_with("/") || is_windows_absolute_pattern(pattern)
}

fn is_windows_absolute_pattern(pattern: &str) -> bool {
  // ex. D:/
  let mut chars = pattern.chars();

  // ensure the first character is alphabetic
  let next_char = chars.next();
  if next_char.is_none() || !next_char.unwrap().is_ascii_alphabetic() {
    return false;
  }

  // skip over the remaining alphabetic characters
  let mut next_char = chars.next();
  while next_char.is_some() && next_char.unwrap().is_ascii_alphabetic() {
    next_char = chars.next();
  }

  // ensure colon
  if next_char != Some(':') {
    return false;
  }

  // now check for the last slash
  let next_char = chars.next();
  matches!(next_char, Some('/'))
}

pub struct GlobMatcherOptions {
  pub case_insensitive: bool,
}

pub struct GlobMatcher {
  include_globset: GlobSet,
  exclude_globset: GlobSet,
}

impl GlobMatcher {
  pub fn new(patterns: &[String], opts: &GlobMatcherOptions) -> Result<GlobMatcher, ErrBox> {
    let mut match_patterns = Vec::new();
    let mut ignore_patterns = Vec::new();
    for pattern in patterns {
      if is_negated_glob(pattern) {
        ignore_patterns.push(pattern[1..].to_string());
      } else {
        match_patterns.push(pattern.to_string());
      }
    }
    Ok(GlobMatcher {
      include_globset: build_glob_set(&match_patterns, opts)?,
      exclude_globset: build_glob_set(&ignore_patterns, opts)?,
    })
  }

  pub fn is_match(&self, pattern: impl AsRef<Path>) -> bool {
    self.include_globset.is_match(&pattern) && !self.exclude_globset.is_match(&pattern)
  }

  pub fn is_ignored(&self, pattern: impl AsRef<Path>) -> bool {
    self.exclude_globset.is_match(&pattern)
  }
}

fn build_glob_set(file_patterns: &[String], opts: &GlobMatcherOptions) -> Result<GlobSet, ErrBox> {
  let mut builder = GlobSetBuilder::new();
  for pattern in file_patterns {
    builder.add(GlobBuilder::new(&pattern).case_insensitive(opts.case_insensitive).build()?);
  }
  return Ok(builder.build().unwrap());
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn it_should_get_if_absolute_pattern() {
    assert_eq!(is_absolute_pattern("test.ts"), false);
    assert_eq!(is_absolute_pattern("!test.ts"), false);
    assert_eq!(is_absolute_pattern("/test.ts"), true);
    assert_eq!(is_absolute_pattern("!/test.ts"), true);
    assert_eq!(is_absolute_pattern("D:/test.ts"), true);
    assert_eq!(is_absolute_pattern("!D:/test.ts"), true);
  }

  #[test]
  fn it_should_get_absolute_globs() {
    assert_eq!(to_absolute_glob("**/*.ts", "/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "/test"), "/test/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "/test/"), "/test/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/test"), "/**/*.ts");
    assert_eq!(to_absolute_glob("/**/*.ts", "/test/"), "/**/*.ts");
    assert_eq!(to_absolute_glob("D:/**/*.ts", "/test/"), "D:/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "D:/"), "D:/**/*.ts");
    assert_eq!(to_absolute_glob(".", "D:\\test"), "D:/test");
    assert_eq!(to_absolute_glob("\\test\\asdf.ts", "D:\\test"), "/test/asdf.ts");
    assert_eq!(to_absolute_glob("!**/*.ts", "D:\\test"), "!D:/test/**/*.ts");
    assert_eq!(to_absolute_glob("///test/**/*.ts", "D:\\test"), "///test/**/*.ts");
    assert_eq!(to_absolute_glob("**/*.ts", "CD:\\"), "CD:/**/*.ts");

    assert_eq!(to_absolute_glob("./test.ts", "/test/"), "/test/test.ts");
    assert_eq!(to_absolute_glob("test.ts", "/test/"), "/test/**/test.ts");
    assert_eq!(to_absolute_glob("*/test.ts", "/test/"), "/test/*/test.ts");
    assert_eq!(to_absolute_glob("*test.ts", "/test/"), "/test/**/*test.ts");
    assert_eq!(to_absolute_glob("**/test.ts", "/test/"), "/test/**/test.ts");
    assert_eq!(to_absolute_glob("/test.ts", "/test/"), "/test.ts");
    assert_eq!(to_absolute_glob("test/", "/test/"), "/test/**/test/");

    assert_eq!(to_absolute_glob("!./test.ts", "/test/"), "!/test/test.ts");
    assert_eq!(to_absolute_glob("!test.ts", "/test/"), "!/test/**/test.ts");
    assert_eq!(to_absolute_glob("!*/test.ts", "/test/"), "!/test/*/test.ts");
    assert_eq!(to_absolute_glob("!*test.ts", "/test/"), "!/test/**/*test.ts");
    assert_eq!(to_absolute_glob("!**/test.ts", "/test/"), "!/test/**/test.ts");
    assert_eq!(to_absolute_glob("!/test.ts", "/test/"), "!/test.ts");
    assert_eq!(to_absolute_glob("!test/", "/test/"), "!/test/**/test/");
    // has a slash in the middle, so it's relative
    assert_eq!(to_absolute_glob("test/test.ts", "/test/"), "/test/test/test.ts");
  }
}

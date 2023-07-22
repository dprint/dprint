use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use parking_lot::Condvar;
use parking_lot::Mutex;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::environment::DirEntry;
use crate::environment::Environment;

use super::GlobMatcher;
use super::GlobMatcherOptions;
use super::GlobPatterns;

#[derive(Debug, Default, Clone)]
pub struct GlobOutput {
  pub file_paths: Vec<PathBuf>,
  pub config_files: Vec<PathBuf>,
}

pub fn glob(environment: &impl Environment, base: impl AsRef<Path>, file_patterns: GlobPatterns) -> Result<GlobOutput> {
  if file_patterns.includes.is_some() && file_patterns.includes.as_ref().unwrap().iter().all(|p| p.is_negated()) {
    // performance improvement (see issue #379)
    log_verbose!(environment, "Skipping negated globs: {:?}", file_patterns.includes);
    return Ok(Default::default());
  }

  let start_instant = std::time::Instant::now();
  log_verbose!(environment, "Globbing: {:?}", file_patterns);

  let glob_matcher = GlobMatcher::new(
    file_patterns,
    &GlobMatcherOptions {
      // make it work the same way on every operating system
      case_sensitive: false,
      base_dir: environment.cwd(),
    },
  )?;

  let start_dir = base.as_ref().to_path_buf();
  let shared_state = Arc::new(SharedState::new(start_dir.clone()));

  // This is a performance improvement to attempt to reduce the time of globbing down
  // to the speed of `fs::read_dir` calls. Essentially, run all the `fs::read_dir` calls
  // on a new thread and do the glob matching on the other thread.
  let read_dir_runner = ReadDirRunner::new(start_dir, environment.clone(), shared_state.clone());
  tokio::task::spawn_blocking(move || read_dir_runner.run());

  // run the glob matching on the current thread (the two threads will communicate with each other)
  let glob_matching_processor = GlobMatchingProcessor::new(shared_state, glob_matcher);
  let results = glob_matching_processor.run()?;

  log_verbose!(environment, "File(s) matched: {:?}", results);
  log_verbose!(environment, "Finished globbing in {}ms", start_instant.elapsed().as_millis());

  Ok(results)
}

enum DirOrConfigEntry {
  Dir(PathBuf),
  File(PathBuf),
  // todo: finish implementing this later
  #[allow(dead_code)]
  Config(PathBuf),
}

const PUSH_DIR_ENTRIES_BATCH_COUNT: usize = 500;

struct ReadDirRunner<TEnvironment: Environment> {
  environment: TEnvironment,
  // todo: finish implementing this later
  #[allow(dead_code)]
  start_dir: PathBuf,
  shared_state: Arc<SharedState>,
}

impl<TEnvironment: Environment> ReadDirRunner<TEnvironment> {
  pub fn new(start_dir: PathBuf, environment: TEnvironment, shared_state: Arc<SharedState>) -> Self {
    Self {
      environment,
      shared_state,
      start_dir,
    }
  }

  pub fn run(&self) {
    while let Some(pending_dirs) = self.get_next_pending_dirs() {
      let mut all_entries = Vec::new();
      for current_dir in pending_dirs.into_iter().flatten() {
        let info_result = self
          .environment
          .dir_info(&current_dir)
          .map_err(|err| anyhow!("Error reading dir '{}': {:#}", current_dir.display(), err));
        match info_result {
          Ok(entries) => {
            if !entries.is_empty() {
              // let maybe_config_file = if current_dir != self.start_dir {
              //   entries
              //     .iter()
              //     .filter_map(|e| match e {
              //       DirEntry::Directory(_) => None,
              //       DirEntry::File { name, path } => {
              //         if matches!(name.to_str(), Some(".dprint.json" | "dprint.json" | ".dprint.jsonc" | "dprint.jsonc")) {
              //           Some(path)
              //         } else {
              //           None
              //         }
              //       }
              //     })
              //     .next()
              // } else {
              //   None
              // };
              // if let Some(config_file) = maybe_config_file {
              //   all_entries.push(DirOrConfigEntry::Config(config_file.clone()));
              // } else {
              all_entries.extend(entries.into_iter().map(|e| match e {
                DirEntry::Directory(path) => DirOrConfigEntry::Dir(path),
                DirEntry::File { path, .. } => DirOrConfigEntry::File(path),
              }));
              // it is much faster to batch these than to hit the lock every time
              if all_entries.len() > PUSH_DIR_ENTRIES_BATCH_COUNT {
                self.push_entries(std::mem::take(&mut all_entries));
              }
              //}
            }
          }
          Err(err) => {
            self.set_glob_error(err);
            return;
          }
        }
      }
      if !all_entries.is_empty() {
        self.push_entries(all_entries);
      }
    }
  }

  fn get_next_pending_dirs(&self) -> Option<Vec<Vec<PathBuf>>> {
    let (ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    loop {
      if !state.pending_dirs.is_empty() {
        state.read_dir_thread_state = ReadDirThreadState::Processing;
        cvar.notify_one();
        return Some(std::mem::take(&mut state.pending_dirs));
      }
      state.read_dir_thread_state = ReadDirThreadState::Waiting;
      cvar.notify_one();
      if matches!(state.processing_thread_state, ProcessingThreadState::Waiting) && state.pending_entries.is_empty() {
        return None;
      } else {
        // wait to be notified by the other thread
        cvar.wait(&mut state);
      }
    }
  }

  fn set_glob_error(&self, error: Error) {
    let (ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.read_dir_thread_state = ReadDirThreadState::Error(error);
    cvar.notify_one();
  }

  fn push_entries(&self, entries: Vec<DirOrConfigEntry>) {
    let (ref lock, ref cvar) = &self.shared_state.inner;
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

  pub fn run(&self) -> Result<GlobOutput> {
    let mut output = GlobOutput::default();

    loop {
      let mut pending_dirs = Vec::new();

      match self.get_next_entries() {
        Ok(None) => return Ok(output),
        Err(err) => return Err(err), // error
        Ok(Some(entries)) => {
          for entry in entries.into_iter().flatten() {
            match entry {
              DirOrConfigEntry::Dir(path) => {
                if !self.glob_matcher.is_dir_ignored(&path) {
                  pending_dirs.push(path);
                }
              }
              DirOrConfigEntry::File(path) => {
                if self.glob_matcher.matches(&path) {
                  output.file_paths.push(path);
                }
              }
              DirOrConfigEntry::Config(path) => {
                output.config_files.push(path);
              }
            }
          }
        }
      }

      self.push_pending_dirs(pending_dirs);
    }
  }

  fn push_pending_dirs(&self, pending_dirs: Vec<PathBuf>) {
    let (ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.pending_dirs.push(pending_dirs);
    cvar.notify_one();
  }

  fn get_next_entries(&self) -> Result<Option<Vec<Vec<DirOrConfigEntry>>>> {
    let (ref lock, ref cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    loop {
      if !state.pending_entries.is_empty() {
        state.processing_thread_state = ProcessingThreadState::Processing;
        return Ok(Some(std::mem::take(&mut state.pending_entries)));
      }
      if !matches!(state.processing_thread_state, ProcessingThreadState::Waiting) {
        state.processing_thread_state = ProcessingThreadState::Waiting;
        cvar.notify_one();
      }
      match &state.read_dir_thread_state {
        ReadDirThreadState::Waiting => {
          if state.pending_dirs.is_empty() {
            return Ok(None);
          } else {
            // wait to be notified by the other thread
            cvar.wait(&mut state);
          }
        }
        ReadDirThreadState::Error(err) => {
          return Err(anyhow!("{:#}", err));
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
  Error(Error),
}

enum ProcessingThreadState {
  Processing,
  Waiting,
}

struct SharedStateInternal {
  pending_dirs: Vec<Vec<PathBuf>>,
  pending_entries: Vec<Vec<DirOrConfigEntry>>,
  read_dir_thread_state: ReadDirThreadState,
  processing_thread_state: ProcessingThreadState,
}

struct SharedState {
  inner: (Mutex<SharedStateInternal>, Condvar),
}

impl SharedState {
  pub fn new(initial_dir: PathBuf) -> Self {
    SharedState {
      inner: (
        Mutex::new(SharedStateInternal {
          processing_thread_state: ProcessingThreadState::Waiting,
          read_dir_thread_state: ReadDirThreadState::Processing,
          pending_dirs: vec![vec![initial_dir]],
          pending_entries: Vec::new(),
        }),
        Condvar::new(),
      ),
    }
  }
}

#[cfg(test)]
mod test {
  use pretty_assertions::assert_eq;

  use super::*;
  use crate::environment::TestEnvironmentBuilder;
  use crate::utils::GlobPattern;

  // `glob` internally uses tokio::spawn_blocking so that's why these
  // are using tokio::test, then that requires async
  #[tokio::test]
  async fn should_glob() {
    let mut environment_builder = TestEnvironmentBuilder::new();
    let mut expected_matches = Vec::new();
    for i in 1..100 {
      environment_builder.write_file(format!("/{}.txt", i), "");
      expected_matches.push(format!("/{}.txt", i));

      environment_builder.write_file(format!("/sub/{}.txt", i), "");
      expected_matches.push(format!("/sub/{}.txt", i));

      environment_builder.write_file(format!("/sub/ignore/{}.txt", i), "");

      environment_builder.write_file(format!("/sub{0}/sub/{0}.txt", i), "");
      expected_matches.push(format!("/sub{0}/sub/{0}.txt", i));

      if i % 2 == 0 {
        environment_builder.write_file(format!("/{}.ps", i), "");
        environment_builder.write_file(format!("/sub/{}.ps", i), "");
        environment_builder.write_file(format!("/sub/ignore/{}.ps", i), "");
        environment_builder.write_file(format!("/sub{0}/sub/{0}.ps", i), "");
      }
    }

    let environment = environment_builder.build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      "/",
      GlobPatterns {
        includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir.clone())]),
        excludes: vec![GlobPattern::new("**/ignore".to_string(), root_dir)],
      },
    )
    .unwrap();
    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    expected_matches.sort();
    assert_eq!(result, expected_matches);
  }

  #[tokio::test]
  async fn should_handle_dir_info_erroring() {
    let environment = TestEnvironmentBuilder::new().build();
    environment.set_dir_info_error(anyhow!("FAILURE"));
    let root_dir = environment.canonicalize("/").unwrap();
    let err_message = glob(
      &environment,
      "/",
      GlobPatterns {
        includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir)]),
        excludes: Vec::new(),
      },
    )
    .err()
    .unwrap();
    assert_eq!(err_message.to_string(), "Error reading dir '/': FAILURE");
  }

  #[tokio::test]
  async fn should_support_excluding_then_including_in_includes() {
    // this allows people to out out of everything then slowly opt back in
    let environment = TestEnvironmentBuilder::new().write_file("/dir/a.txt", "").write_file("/dir/b.txt", "").build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      "/",
      GlobPatterns {
        includes: Some(vec![
          GlobPattern::new("!**/*.*".to_string(), root_dir.clone()),
          GlobPattern::new("**/a.txt".to_string(), root_dir),
        ]),
        excludes: Vec::new(),
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/dir/a.txt"]);
  }

  #[tokio::test]
  async fn should_support_including_then_excluding_then_including() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/dir/a.json", "")
      .write_file("/dir/b.json", "")
      .build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      "/",
      GlobPatterns {
        includes: Some(vec![
          GlobPattern::new("**/*.json".to_string(), root_dir.clone()),
          GlobPattern::new("!**/*.json".to_string(), root_dir.clone()),
          GlobPattern::new("**/a.json".to_string(), root_dir),
        ]),
        excludes: Vec::new(),
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/dir/a.json"]);
  }

  #[tokio::test]
  async fn excluding_dir_but_including_sub_dir() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/test/a/a.json", "")
      .write_file("/test/a/b/b.json", "")
      .write_file("/test/test.json", "")
      .build();
    let test_dir = environment.canonicalize("/test/").unwrap();
    let result = glob(
      &environment,
      "/test/",
      GlobPatterns {
        includes: Some(vec![
          GlobPattern::new("**/*.json".to_string(), test_dir.clone()),
          GlobPattern::new("!a/**/*.json".to_string(), test_dir.clone()),
          GlobPattern::new("a/b/**/*.json".to_string(), test_dir),
        ]),
        excludes: Vec::new(),
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/test/a/b/b.json", "/test/test.json"]);
  }

  #[tokio::test]
  async fn excluding_dir_but_including_sub_dir_case_2() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/dir/a/a.txt", "")
      .write_file("/dir/b/b.txt", "")
      .build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      "/",
      GlobPatterns {
        includes: Some(vec![
          GlobPattern::new("**/*.*".to_string(), root_dir.clone()),
          GlobPattern::new("!dir/a/**/*".to_string(), root_dir.clone()),
          GlobPattern::new("dir/b/b/**/*".to_string(), root_dir),
        ]),
        excludes: Vec::new(),
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/dir/b/b.txt"]);
  }
}

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use parking_lot::Condvar;
use parking_lot::Mutex;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::arg_parser::ConfigDiscovery;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::DirEntry;
use crate::environment::Environment;
use crate::utils::gitignore::GitIgnoreTree;

use super::ExcludeMatchDetail;
use super::GlobMatcher;
use super::GlobMatcherOptions;
use super::GlobMatchesDetail;
use super::GlobPatterns;

#[derive(Debug, Default, Clone)]
pub struct GlobOutput {
  pub file_paths: Vec<PathBuf>,
  pub config_files: Vec<PathBuf>,
}

pub struct GlobOptions {
  /// The directory to start searching from.
  pub start_dir: PathBuf,
  /// Whether to enable configuration discovery.
  pub config_discovery: ConfigDiscovery,
  /// The file patterns to use for globbing.
  pub file_patterns: GlobPatterns,
  /// The directory to use as the base for the patterns.
  /// Generally you want this to be the directory of the config file.
  pub pattern_base: CanonicalizedPathBuf,
}

pub fn glob(environment: &impl Environment, opts: GlobOptions) -> Result<GlobOutput> {
  if opts
    .file_patterns
    .arg_includes
    .as_ref()
    .map(|p| p.iter().all(|p| p.is_negated()))
    .unwrap_or(false)
  {
    // performance improvement (see issue #379)
    log_debug!(environment, "Skipping negated globs: {:?}", opts.file_patterns.arg_includes);
    return Ok(Default::default());
  }

  let start_instant = std::time::Instant::now();
  log_debug!(environment, "Globbing: {:?}", opts.file_patterns);

  let git_ignore_tree = GitIgnoreTree::new(environment.clone(), opts.file_patterns.include_paths());
  let glob_matcher = GlobMatcher::new(
    opts.file_patterns,
    &GlobMatcherOptions {
      // make it work the same way on every operating system
      case_sensitive: false,
      base_dir: opts.pattern_base,
    },
  )?;

  let shared_state = Arc::new(SharedState::new(opts.start_dir.clone()));

  // This is a performance improvement to attempt to reduce the time of globbing down
  // to the speed of `fs::read_dir` calls. Essentially, run all the `fs::read_dir` calls
  // on a new thread and do the glob matching on the other thread.
  let read_dir_runner = ReadDirRunner::new(
    environment.clone(),
    shared_state.clone(),
    ReadDirRunnerOptions {
      start_dir: opts.start_dir,
      config_discovery: opts.config_discovery,
    },
  );
  dprint_core::async_runtime::spawn_blocking(move || read_dir_runner.run());

  // run the glob matching on the current thread (the two threads will communicate with each other)
  let mut glob_matching_processor = GlobMatchingProcessor::new(shared_state, glob_matcher, git_ignore_tree);
  let results = glob_matching_processor.run()?;

  log_debug!(environment, "File(s) matched: {:?}", results);
  log_debug!(environment, "Finished globbing in {}ms", start_instant.elapsed().as_millis());

  Ok(results)
}

struct DirEntries {
  path: PathBuf,
  entries: Vec<DirOrConfigEntry>,
}

enum DirOrConfigEntry {
  Dir(PathBuf),
  File(PathBuf),
  // todo: get rid of this from here probably
  Config(PathBuf),
}

const PUSH_DIR_ENTRIES_BATCH_COUNT: usize = 500;

struct ReadDirRunnerOptions {
  start_dir: PathBuf,
  config_discovery: ConfigDiscovery,
}

struct ReadDirRunner<TEnvironment: Environment> {
  environment: TEnvironment,
  shared_state: Arc<SharedState>,
  options: ReadDirRunnerOptions,
}

impl<TEnvironment: Environment> ReadDirRunner<TEnvironment> {
  pub fn new(environment: TEnvironment, shared_state: Arc<SharedState>, options: ReadDirRunnerOptions) -> Self {
    Self {
      environment,
      shared_state,
      options,
    }
  }

  pub fn run(&self) {
    while let Some(pending_dirs) = self.get_next_pending_dirs() {
      let mut pending_count = 0;
      let mut all_entries = Vec::new();
      for current_dir in pending_dirs.into_iter().flatten() {
        let info_result = self.environment.dir_info(&current_dir);
        let entries = match info_result {
          Ok(entries) => {
            if entries.is_empty() {
              continue;
            }
            let maybe_config_file = if self.options.config_discovery.traverse_descendants() && current_dir != self.options.start_dir {
              entries
                .iter()
                .filter_map(|e| match e {
                  DirEntry::Directory(_) => None,
                  DirEntry::File { name, path } => {
                    if matches!(name.to_str(), Some(".dprint.json" | "dprint.json" | ".dprint.jsonc" | "dprint.jsonc")) {
                      Some(path)
                    } else {
                      None
                    }
                  }
                })
                .next()
            } else {
              None
            };
            if let Some(config_file) = maybe_config_file {
              vec![DirOrConfigEntry::Config(config_file.clone())]
            } else {
              entries
                .into_iter()
                .map(|e| match e {
                  DirEntry::Directory(path) => DirOrConfigEntry::Dir(path),
                  DirEntry::File { path, .. } => DirOrConfigEntry::File(path),
                })
                .collect::<Vec<_>>()
            }
          }
          Err(err) => {
            let ignore_error = is_system_volume_error(&current_dir, &err);
            if ignore_error {
              continue;
            }
            if err.kind() == std::io::ErrorKind::PermissionDenied {
              log_warn!(self.environment, "WARNING: Ignoring directory. Permission denied: {}", current_dir.display());
              continue;
            } else {
              self.set_glob_error(anyhow!("Error reading dir '{}': {:#}", current_dir.display(), err));
              return;
            }
          }
        };
        pending_count += entries.len();
        all_entries.push(DirEntries { path: current_dir, entries });
        // it is much faster to batch these than to hit the lock every time
        if pending_count > PUSH_DIR_ENTRIES_BATCH_COUNT {
          self.push_entries(std::mem::take(&mut all_entries));
          pending_count = 0;
        }
      }
      if !all_entries.is_empty() {
        self.push_entries(all_entries);
      }
    }
  }

  fn get_next_pending_dirs(&self) -> Option<Vec<Vec<PathBuf>>> {
    let (lock, cvar) = &self.shared_state.inner;
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
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.read_dir_thread_state = ReadDirThreadState::Error(error);
    cvar.notify_one();
  }

  fn push_entries(&self, entries: Vec<DirEntries>) {
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.pending_entries.push(entries);
    cvar.notify_one();
  }
}

fn is_system_volume_error(dir_path: &Path, err: &std::io::Error) -> bool {
  // ignore any access denied errors for the system volume information
  cfg!(target_os = "windows")
    && matches!(err.raw_os_error(), Some(5))
    && matches!(dir_path.file_name().and_then(|f| f.to_str()), Some("System Volume Information"))
}

struct GlobMatchingProcessor<TEnvironment: Environment> {
  shared_state: Arc<SharedState>,
  glob_matcher: GlobMatcher,
  git_ignore_tree: GitIgnoreTree<TEnvironment>,
}

impl<TEnvironment: Environment> GlobMatchingProcessor<TEnvironment> {
  pub fn new(shared_state: Arc<SharedState>, glob_matcher: GlobMatcher, git_ignore_tree: GitIgnoreTree<TEnvironment>) -> Self {
    Self {
      shared_state,
      glob_matcher,
      git_ignore_tree,
    }
  }

  pub fn run(&mut self) -> Result<GlobOutput> {
    let mut output = GlobOutput::default();

    loop {
      let mut pending_dirs = Vec::new();

      match self.get_next_entries() {
        Ok(None) => return Ok(output),
        Err(err) => return Err(err), // error
        Ok(Some(entries)) => {
          for dir in entries.into_iter().flatten() {
            let gitignore = self.git_ignore_tree.get_resolved_git_ignore_for_dir_children(&dir.path);
            for entry in dir.entries {
              match entry {
                DirOrConfigEntry::Dir(path) => {
                  let is_ignored = match self.glob_matcher.is_dir_ignored(&path) {
                    ExcludeMatchDetail::Excluded => true,
                    ExcludeMatchDetail::OptedOutExclude => false,
                    ExcludeMatchDetail::NotExcluded => match &gitignore {
                      Some(gitignore) => {
                        gitignore.is_ignored(&path, /* is dir */ true)
                      }
                      None => false,
                    },
                  } || path.file_name().map(|f| f == ".git").unwrap_or(false);
                  if !is_ignored {
                    pending_dirs.push(path);
                  }
                }
                DirOrConfigEntry::File(path) => {
                  let is_matched = match self.glob_matcher.matches_detail(&path) {
                    GlobMatchesDetail::Excluded => false,
                    GlobMatchesDetail::Matched => match &gitignore {
                      Some(gitignore) => {
                        !gitignore.is_ignored(&path, /* is dir */ false)
                      }
                      None => true,
                    },
                    GlobMatchesDetail::MatchedOptedOutExclude => true,
                    GlobMatchesDetail::NotMatched => false,
                  };
                  if is_matched {
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
      }

      self.push_pending_dirs(pending_dirs);
    }
  }

  fn push_pending_dirs(&self, pending_dirs: Vec<PathBuf>) {
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.pending_dirs.push(pending_dirs);
    cvar.notify_one();
  }

  fn get_next_entries(&self) -> Result<Option<Vec<Vec<DirEntries>>>> {
    let (lock, cvar) = &self.shared_state.inner;
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
  pending_entries: Vec<Vec<DirEntries>>,
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
    // ignores .git folder
    environment_builder.write_file("/.git/data.txt", "");
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
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: None,
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir.clone())]),
          arg_excludes: None,
          config_excludes: vec![GlobPattern::new("**/ignore".to_string(), root_dir)],
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
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
    environment.set_dir_info_error(std::io::Error::new(std::io::ErrorKind::Other, "FAILURE"));
    let root_dir = environment.canonicalize("/").unwrap();
    let err_message = glob(
      &environment,
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: None,
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir)]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
      },
    )
    .err()
    .unwrap();
    assert_eq!(err_message.to_string(), "Error reading dir '/': FAILURE");
  }

  #[tokio::test]
  async fn should_ignore_permission_denied_error() {
    let environment = TestEnvironmentBuilder::new().build();
    environment.set_dir_info_error(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied"));
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: None,
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir)]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
      },
    );
    assert!(result.is_ok());
    assert_eq!(
      environment.take_stderr_messages(),
      vec!["WARNING: Ignoring directory. Permission denied: /".to_string()]
    );
  }

  #[tokio::test]
  async fn should_support_excluding_then_including_in_includes() {
    // this allows people to out out of everything then slowly opt back in
    let environment = TestEnvironmentBuilder::new().write_file("/dir/a.txt", "").write_file("/dir/b.txt", "").build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: None,
          config_includes: Some(vec![
            GlobPattern::new("!**/*.*".to_string(), root_dir.clone()),
            GlobPattern::new("**/a.txt".to_string(), root_dir),
          ]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
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
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: None,
          config_includes: Some(vec![
            GlobPattern::new("**/*.json".to_string(), root_dir.clone()),
            GlobPattern::new("!**/*.json".to_string(), root_dir.clone()),
            GlobPattern::new("**/a.json".to_string(), root_dir),
          ]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
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
      GlobOptions {
        start_dir: PathBuf::from("/test/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: None,
          config_includes: Some(vec![
            GlobPattern::new("**/*.json".to_string(), test_dir.clone()),
            GlobPattern::new("!a/**/*.json".to_string(), test_dir.clone()),
            GlobPattern::new("a/b/**/*.json".to_string(), test_dir),
          ]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/test/"),
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
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: None,
          config_includes: Some(vec![
            GlobPattern::new("**/*.*".to_string(), root_dir.clone()),
            GlobPattern::new("!dir/a/**/*".to_string(), root_dir.clone()),
            GlobPattern::new("dir/b/b/**/*".to_string(), root_dir),
          ]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/dir/b/b.txt"]);
  }
}

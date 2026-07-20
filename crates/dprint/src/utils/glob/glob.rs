use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use parking_lot::Condvar;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::arg_parser::ConfigDiscovery;
use crate::configuration::POSSIBLE_CONFIG_FILE_NAMES;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::DirEntry;
use crate::environment::Environment;
use crate::utils::gitignore::DirEntriesHint;
use crate::utils::gitignore::GitIgnoreTree;
use crate::utils::gitignore::GitIgnoreTreeOptions;
use crate::utils::gitignore::resolve_global_gitignore_lines;

use super::ExcludeMatchDetail;
use super::GlobMatcher;
use super::GlobMatcherOptions;
use super::GlobMatchesDetail;
use super::GlobPattern;
use super::GlobPatterns;
use super::escape_glob_text;
use super::is_pattern;
use super::non_negated_glob;
use super::unescape_glob_text;

#[derive(Debug, Default, Clone)]
pub struct GlobOutput {
  pub file_paths: Vec<PathBuf>,
  pub config_files: Vec<PathBuf>,
  /// CLI paths and patterns that are outside the pattern base directory.
  /// The caller resolves the config file to use for these separately.
  pub outside_base_paths: Vec<OutsideBasePath>,
}

/// A CLI path or glob pattern that is outside the pattern base directory.
#[derive(Debug, Clone)]
pub struct OutsideBasePath {
  /// The directory to start searching for the governing config file from.
  pub config_search_dir: PathBuf,
  /// The absolute path or pattern to resolve in the new scope.
  pub include_pattern: String,
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
  /// Whether to disable respecting .gitignore files.
  pub no_gitignore: bool,
}

pub fn glob(environment: &impl Environment, mut opts: GlobOptions) -> Result<GlobOutput> {
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

  // resolve literal (non-glob) CLI paths directly against the file system so
  // they can be matched without directory traversal (this also allows matching
  // paths outside the directory the traversal starts in, like ../file.txt)
  let mut literal_arg_paths = extract_literal_arg_paths(environment, &mut opts.file_patterns, &opts.pattern_base);
  rewrite_literal_exclude_paths(environment, &mut opts.file_patterns, &opts.pattern_base);
  extract_outside_arg_patterns(&mut opts.file_patterns, &opts.pattern_base, &mut literal_arg_paths.outside_base_paths);
  let mut run_traversal = requires_traversal(&opts.file_patterns);
  if run_traversal {
    opts.start_dir = expand_start_dir_for_arg_patterns(opts.start_dir, &opts.file_patterns, &opts.pattern_base);
  } else {
    log_debug!(environment, "Skipping traversal because the CLI args were all file paths.");
  }

  let mut git_ignore_tree = if opts.no_gitignore {
    None
  } else {
    Some(GitIgnoreTree::new(
      environment.clone(),
      GitIgnoreTreeOptions {
        include_paths: opts.file_patterns.include_paths(),
        global_gitignore_lines: resolve_global_gitignore_lines(environment),
      },
    ))
  };
  let glob_matcher = GlobMatcher::new(
    opts.file_patterns,
    &GlobMatcherOptions {
      // make it work the same way on every operating system
      case_sensitive: true,
      base_dir: opts.pattern_base.clone(),
    },
  )?;

  let mut output = GlobOutput {
    outside_base_paths: literal_arg_paths.outside_base_paths,
    ..Default::default()
  };

  let discover_configs = opts.config_discovery.traverse_descendants();
  let mut config_file_finder = DirConfigFileFinder::new(environment);

  // check the directories between the pattern base and the start directory the
  // same way a traversal descending from the pattern base would so matching
  // works the same regardless of the directory dprint is run from
  if run_traversal && opts.start_dir != opts.pattern_base.as_ref() && opts.start_dir.starts_with(opts.pattern_base.as_ref()) {
    match check_dir_chain(
      &glob_matcher,
      &mut git_ignore_tree,
      discover_configs.then_some(&mut config_file_finder),
      opts.pattern_base.as_ref(),
      &opts.start_dir,
    ) {
      DirChainResult::Matched => {}
      DirChainResult::Excluded => {
        log_debug!(environment, "Skipping traversal because the start directory is excluded.");
        run_traversal = false;
      }
      DirChainResult::HasConfigFile(config_file) => {
        // the sub scope created for the config file handles this directory
        log_debug!(environment, "Skipping traversal because the start directory has its own config file.");
        push_dedup_config_file(&mut output.config_files, config_file);
        run_traversal = false;
      }
    }
  }

  // match the literal file paths (a gitignored file is still matched when
  // explicitly specified, but not when one of its ancestor directories is
  // gitignored because a traversal wouldn't descend into the directory)
  for file_path in literal_arg_paths.file_paths {
    if run_traversal && file_path.starts_with(&opts.start_dir) {
      continue; // the traversal will find this file
    }
    // examine the file's ancestor directories top down the same way a
    // traversal would: an excluded directory excludes everything within it
    // and a directory with its own config file is handled by the sub scope
    // created for that config file instead of the current scope
    if let Some(parent) = file_path.parent()
      && parent.starts_with(opts.pattern_base.as_ref())
    {
      match check_dir_chain(
        &glob_matcher,
        &mut git_ignore_tree,
        discover_configs.then_some(&mut config_file_finder),
        opts.pattern_base.as_ref(),
        parent,
      ) {
        DirChainResult::Matched => {}
        DirChainResult::Excluded => continue,
        DirChainResult::HasConfigFile(config_file) => {
          push_dedup_config_file(&mut output.config_files, config_file);
          continue;
        }
      }
    }
    if glob_matcher.matches(&file_path) {
      output.file_paths.push(file_path);
    }
  }

  if run_traversal {
    let shared_state = Arc::new(SharedState::new(opts.start_dir.clone()));

    // This is a performance improvement to attempt to reduce the time of globbing down
    // to the speed of `fs::read_dir` calls. Essentially, run all the `fs::read_dir` calls
    // on separate threads and do the glob matching on the current thread.
    //
    // Reading directories is I/O bound, so spreading the reads across several threads
    // saturates the disk far better than a single reader can. See issue #1001.
    let read_dir_thread_count = resolve_read_dir_thread_count(environment);
    log_debug!(environment, "Reading directories on {} thread(s)", read_dir_thread_count);
    let read_dir_runner = Arc::new(ReadDirRunner::new(
      environment.clone(),
      shared_state.clone(),
      ReadDirRunnerOptions {
        start_dir: opts.start_dir,
        config_discovery: opts.config_discovery,
        thread_count: read_dir_thread_count,
      },
    ));
    for _ in 0..read_dir_thread_count {
      let read_dir_runner = read_dir_runner.clone();
      dprint_core::async_runtime::spawn_blocking(move || read_dir_runner.run());
    }

    // run the glob matching on the current thread (it communicates with the reader threads)
    let mut glob_matching_processor = GlobMatchingProcessor::new(shared_state, glob_matcher, git_ignore_tree);
    let results = glob_matching_processor.run()?;
    output.file_paths.extend(results.file_paths);
    output.config_files.extend(results.config_files);
  }

  log_debug!(environment, "File(s) matched: {:?}", output);
  log_debug!(environment, "Finished globbing in {}ms", start_instant.elapsed().as_millis());

  Ok(output)
}

struct LiteralArgPaths {
  /// Files to match in the current scope.
  file_paths: Vec<PathBuf>,
  /// Existing paths outside the pattern base directory.
  outside_base_paths: Vec<OutsideBasePath>,
}

/// Resolves the positive literal CLI patterns against the file system.
///
/// Existing files are returned so they can be matched directly without any
/// directory traversal, and patterns for existing directories are expanded to
/// match everything within them (ex. `dprint fmt some_dir`). A glob-like
/// pattern whose text names an existing path is treated as that path instead
/// of a glob (ex. `dprint fmt routes/[id].svelte`). Paths outside the pattern
/// base directory are returned separately because the config file to use for
/// them needs to be resolved by the caller.
fn extract_literal_arg_paths(environment: &impl Environment, file_patterns: &mut GlobPatterns, pattern_base: &CanonicalizedPathBuf) -> LiteralArgPaths {
  let mut seen = HashSet::new();
  let mut result = LiteralArgPaths {
    file_paths: Vec::new(),
    outside_base_paths: Vec::new(),
  };
  for pattern in file_patterns.arg_includes.iter_mut().flatten() {
    if pattern.is_negated() {
      // a negated arg with an existing literal name should skip that path
      // instead of matching as a glob (ex. `!routes/[id].svelte`)
      rewrite_literal_arg_pattern(environment, pattern, pattern_base);
      continue;
    }
    if !could_be_literal_path(&pattern.relative_pattern) {
      continue;
    }
    let relative_path = pattern.relative_pattern.strip_prefix("./").unwrap_or(&pattern.relative_pattern);
    let relative_path = relative_path.trim_end_matches('/');
    // unescape so escaped glob characters resolve to the actual path
    // (ex. `\[a\]/file.js`)
    let relative_path = unescape_glob_text(relative_path);
    let (path, is_file) = if relative_path.is_empty() {
      // the pattern resolved to its base directory itself (ex. `dprint fmt ..`)
      (pattern.base_dir.as_ref().to_path_buf(), false)
    } else {
      // use platform-style separators so the resolved paths are
      // consistent with the paths a traversal produces
      let path = if cfg!(windows) {
        pattern.base_dir.join(relative_path.replace('/', "\\"))
      } else {
        pattern.base_dir.join(relative_path.as_ref())
      };
      let is_file = environment.path_is_file(&path);
      if !is_file && !environment.path_exists(&path) {
        // a glob-like pattern stays a glob when nothing has its literal name
        continue;
      }
      // resolve symlinks and casing differences so the path gets classified
      // and matched based on where it actually is on the file system
      let path = match environment.canonicalize(&path) {
        Ok(canonical) => canonical.into_path_buf(),
        Err(_) => path,
      };
      (path, is_file)
    };
    if !is_file && pattern_base.as_ref().starts_with(&path) {
      // a directory at or above the pattern base directory, so match
      // everything in the current scope
      if path != *pattern_base.as_ref() && seen.insert(path.clone()) {
        // also resolve the parts of the ancestor directory outside the
        // current scope separately (ex. sibling directories with their own
        // config files)
        result.outside_base_paths.push(OutsideBasePath {
          config_search_dir: path.clone(),
          include_pattern: path.to_string_lossy().into_owned(),
        });
      }
      pattern.relative_pattern = "**".to_string();
      pattern.base_dir = pattern_base.clone();
    } else if !path.starts_with(pattern_base.as_ref()) {
      if seen.insert(path.clone()) {
        result.outside_base_paths.push(OutsideBasePath {
          config_search_dir: if is_file {
            path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.clone())
          } else {
            path.clone()
          },
          include_pattern: path.to_string_lossy().into_owned(),
        });
      }
      // escape so the retained pattern is treated as a literal path instead
      // of a glob (the outside scope resolves the path itself)
      pattern.relative_pattern = format!("./{}", escape_glob_text(&relative_path));
    } else {
      // rewrite the pattern to the canonicalized path so the matcher's
      // literal path set agrees with the resolved path (escaping so glob
      // characters in the path match literally)
      let relative = path.strip_prefix(pattern_base.as_ref()).unwrap().to_string_lossy().replace('\\', "/");
      pattern.base_dir = pattern_base.clone();
      if is_file {
        pattern.relative_pattern = format!("./{}", escape_glob_text(&relative));
        if seen.insert(path.clone()) {
          result.file_paths.push(path);
        }
      } else {
        // an existing directory, so match everything within it
        pattern.relative_pattern = format!("./{}/**", escape_glob_text(&relative));
      }
    }
  }
  result
}

/// Rewrites the CLI arg patterns whose text names an existing path to match
/// that path literally, for matchers built without a full `glob()` (ex.
/// `dprint fmt --stdin`). Within `glob()` the positive include args are
/// instead handled by `extract_literal_arg_paths` because they additionally
/// resolve directory contents and outside paths.
pub fn rewrite_literal_arg_patterns(environment: &impl Environment, file_patterns: &mut GlobPatterns, pattern_base: &CanonicalizedPathBuf) {
  for pattern in file_patterns.arg_includes.iter_mut().flatten() {
    rewrite_literal_arg_pattern(environment, pattern, pattern_base);
  }
  rewrite_literal_exclude_paths(environment, file_patterns, pattern_base);
}

/// Rewrites the exclude args whose text names an existing path to match that
/// path literally, the same way include args are resolved (ex. `--excludes
/// "[a]"` excludes a directory named `[a]` instead of matching as a character
/// class).
fn rewrite_literal_exclude_paths(environment: &impl Environment, file_patterns: &mut GlobPatterns, pattern_base: &CanonicalizedPathBuf) {
  for pattern in file_patterns.arg_excludes.iter_mut().flatten() {
    rewrite_literal_arg_pattern(environment, pattern, pattern_base);
  }
}

/// Rewrites a single arg pattern to match literally when its text names an
/// existing path, preserving any negation (ex. `!routes/[id].svelte` skips
/// the file with that name instead of matching as a character class).
pub fn rewrite_literal_arg_pattern(environment: &impl Environment, pattern: &mut GlobPattern, pattern_base: &CanonicalizedPathBuf) {
  let is_negated = pattern.is_negated();
  let text = non_negated_glob(&pattern.relative_pattern);
  if !is_pattern(text) || !could_be_literal_path(text) {
    return;
  }
  let relative_path = text.strip_prefix("./").unwrap_or(text);
  let relative_path = relative_path.trim_end_matches('/');
  if relative_path.is_empty() {
    return;
  }
  let path = if cfg!(windows) {
    pattern.base_dir.join(relative_path.replace('/', "\\"))
  } else {
    pattern.base_dir.join(relative_path)
  };
  if !environment.path_exists(&path) {
    return;
  }
  let canonical_path = match environment.canonicalize(&path) {
    Ok(canonical) => canonical.into_path_buf(),
    Err(_) => path,
  };
  // rebase onto the pattern base when the canonical path is within it,
  // otherwise keep the uncanonicalized name (ex. a symlink pointing outside
  // the base still gets excluded by the name a traversal encounters)
  let (base_dir, relative) = match canonical_path.strip_prefix(pattern_base.as_ref()) {
    Ok(relative) if !relative.as_os_str().is_empty() => (pattern_base.clone(), relative.to_string_lossy().replace('\\', "/")),
    _ => (pattern.base_dir.clone(), relative_path.to_string()),
  };
  pattern.base_dir = base_dir;
  pattern.relative_pattern = format!("{}./{}", if is_negated { "!" } else { "" }, escape_glob_text(&relative));
}

/// Gets whether the pattern's text could name a literal path on the file
/// system and so is worth checking for existence.
///
/// Patterns with `*` or `?` are always treated as globs (`*` isn't even
/// allowed in Windows file names), but `[` and `{` are valid in file names
/// (ex. `routes/[id].svelte`), so those are resolved against the file system
/// (see issues #552, #920, #947).
fn could_be_literal_path(pattern: &str) -> bool {
  let mut was_last_escape = false;
  for c in pattern.chars() {
    if !was_last_escape && matches!(c, '*' | '?') {
      return false;
    }
    was_last_escape = matches!(c, '\\');
  }
  true
}

/// Extracts the positive CLI glob patterns based outside the pattern base
/// directory (ex. `dprint fmt ../other/**` when `../other` is outside the
/// config's directory or on another drive). They can never match anything in
/// the current scope, so the caller resolves the config file to use for them
/// separately the same way it does for literal paths outside the base. A
/// pattern based at an ancestor of the base is kept for the current scope and
/// additionally resolved separately for the parts outside the scope.
fn extract_outside_arg_patterns(file_patterns: &mut GlobPatterns, pattern_base: &CanonicalizedPathBuf, outside_base_paths: &mut Vec<OutsideBasePath>) {
  let Some(includes) = &mut file_patterns.arg_includes else {
    return;
  };
  includes.retain(|pattern| {
    if !is_positive_glob_pattern(pattern) {
      return true;
    }
    let deepest_base = pattern.clone().into_deepest_base().base_dir;
    if deepest_base.starts_with(pattern_base) {
      return true; // only matches within the scope
    }
    outside_base_paths.push(OutsideBasePath {
      config_search_dir: deepest_base.as_ref().to_path_buf(),
      include_pattern: pattern.as_absolute_pattern_text(),
    });
    // a pattern based at an ancestor directory also matches within the scope
    pattern_base.starts_with(&deepest_base)
  });
}

/// Gets whether resolving the patterns requires traversing the file system.
///
/// Traversal isn't necessary when the CLI args are all literal file paths
/// because those are resolved directly against the file system.
fn requires_traversal(file_patterns: &GlobPatterns) -> bool {
  match &file_patterns.arg_includes {
    // no CLI paths were provided, so traverse for the config includes
    None => true,
    Some(includes) => includes.iter().any(is_positive_glob_pattern),
  }
}

/// Gets whether the pattern is a non-negated glob pattern
/// as opposed to a literal path.
fn is_positive_glob_pattern(pattern: &GlobPattern) -> bool {
  !pattern.is_negated() && is_pattern(&pattern.relative_pattern)
}

/// Moves the traversal start directory up to the nearest ancestor directory
/// containing the positive CLI glob patterns so files outside the start
/// directory can be found (ex. `dprint fmt ../sub_dir/**/*.ts`). This stays
/// fast because the traversal quickly prunes directories the patterns can't
/// match (see `GlobPattern::matches_dir_for_traversal`).
fn expand_start_dir_for_arg_patterns(mut start_dir: PathBuf, file_patterns: &GlobPatterns, pattern_base: &CanonicalizedPathBuf) -> PathBuf {
  for pattern in file_patterns.arg_includes.iter().flatten() {
    if !is_positive_glob_pattern(pattern) {
      continue;
    }
    let deepest_base = pattern.clone().into_deepest_base().base_dir;
    if !deepest_base.starts_with(pattern_base) {
      continue;
    }
    // move the start dir up (bounded by the pattern base) until it contains
    // the pattern's base directory
    while !deepest_base.as_ref().starts_with(&start_dir) {
      match start_dir.parent() {
        Some(parent) if start_dir != *pattern_base.as_ref() => start_dir = parent.to_path_buf(),
        _ => break,
      }
    }
  }
  start_dir
}

enum DirChainResult {
  /// Nothing along the chain prevents matching within the directory.
  Matched,
  /// A directory along the chain is excluded or gitignored.
  Excluded,
  /// A directory along the chain has its own config file, so the sub scope
  /// created for that config file handles everything within it.
  HasConfigFile(PathBuf),
}

/// Checks the directories between the base directory (exclusive) and the
/// provided directory (inclusive) top down the same way a traversal
/// descending into them would.
fn check_dir_chain<TEnvironment: Environment>(
  glob_matcher: &GlobMatcher,
  git_ignore_tree: &mut Option<GitIgnoreTree<TEnvironment>>,
  mut config_file_finder: Option<&mut DirConfigFileFinder<'_, TEnvironment>>,
  base_dir: &Path,
  dir: &Path,
) -> DirChainResult {
  for dir in dirs_from_base_to(base_dir, dir) {
    if dir.file_name().is_some_and(|f| f == ".git") {
      return DirChainResult::Excluded;
    }
    match glob_matcher.is_dir_ignored(dir) {
      ExcludeMatchDetail::Excluded => return DirChainResult::Excluded,
      ExcludeMatchDetail::OptedOutExclude => {}
      ExcludeMatchDetail::NotExcluded => {
        if let Some(tree) = git_ignore_tree.as_mut()
          && let Some(gitignore) = tree.get_resolved_git_ignore_for_file(dir)
          && gitignore.is_ignored(dir, /* is dir */ true)
        {
          return DirChainResult::Excluded;
        }
      }
    }
    if let Some(finder) = config_file_finder.as_deref_mut()
      && let Some(config_file) = finder.find(dir)
    {
      return DirChainResult::HasConfigFile(config_file);
    }
  }
  DirChainResult::Matched
}

/// Gets the directories between the base directory (exclusive) and the
/// provided directory (inclusive) ordered top down.
fn dirs_from_base_to<'a>(base_dir: &Path, dir: &'a Path) -> Vec<&'a Path> {
  let mut dirs = dir.ancestors().take_while(|ancestor| *ancestor != base_dir).collect::<Vec<_>>();
  dirs.reverse();
  dirs
}

/// Adds a config file found along a directory chain, deduplicating because
/// multiple file paths often resolve to the same config file.
fn push_dedup_config_file(config_files: &mut Vec<PathBuf>, config_file: PathBuf) {
  if !config_files.contains(&config_file) {
    config_files.push(config_file);
  }
}

/// Finds the dprint config file within a directory, caching the result per
/// directory because multiple file paths often share ancestor directories.
struct DirConfigFileFinder<'a, TEnvironment: Environment> {
  environment: &'a TEnvironment,
  cache: HashMap<PathBuf, Option<PathBuf>>,
}

impl<'a, TEnvironment: Environment> DirConfigFileFinder<'a, TEnvironment> {
  pub fn new(environment: &'a TEnvironment) -> Self {
    Self {
      environment,
      cache: Default::default(),
    }
  }

  pub fn find(&mut self, dir: &Path) -> Option<PathBuf> {
    if let Some(result) = self.cache.get(dir) {
      return result.clone();
    }
    let result = POSSIBLE_CONFIG_FILE_NAMES
      .iter()
      .map(|file_name| dir.join(file_name))
      .find(|path| self.environment.path_is_file(path));
    self.cache.insert(dir.to_path_buf(), result.clone());
    result
  }
}

/// Default number of threads used for reading directories.
///
/// Reading is I/O bound rather than CPU bound, so this is a small fixed value
/// instead of a function of the CPU count: a handful of threads saturate the
/// disk even on a machine with few cores, while going much higher regresses
/// (see the measurements in issue #1001). It's deliberately independent of
/// `DPRINT_MAX_THREADS`, which limits the CPU threads used for formatting.
const DEFAULT_READ_DIR_THREAD_COUNT: usize = 8;

/// Resolves how many threads to use for reading directories.
///
/// Defaults to [`DEFAULT_READ_DIR_THREAD_COUNT`] and can be overridden via the
/// `DPRINT_GLOB_READ_THREADS` environment variable.
fn resolve_read_dir_thread_count(environment: &impl Environment) -> usize {
  if let Some(count) = environment
    .env_var("DPRINT_GLOB_READ_THREADS")
    .and_then(|v| v.to_str().and_then(|v| v.parse::<usize>().ok()))
  {
    return count.max(1);
  }
  DEFAULT_READ_DIR_THREAD_COUNT
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

/// Derives a gitignore resolution hint from an already-read directory listing.
fn dir_entries_hint(entries: &[DirOrConfigEntry]) -> DirEntriesHint {
  let mut hint = DirEntriesHint {
    has_git: false,
    has_gitignore: false,
  };
  for entry in entries {
    match entry {
      DirOrConfigEntry::Dir(path) | DirOrConfigEntry::File(path) => {
        match path.file_name().and_then(|f| f.to_str()) {
          // `.gitignore` is a file and `.git` is usually a directory (a file in worktrees)
          Some(".gitignore") => hint.has_gitignore = true,
          Some(".git") => hint.has_git = true,
          _ => continue,
        }
      }
      DirOrConfigEntry::Config(_) => continue,
    }
    if hint.has_gitignore && hint.has_git {
      break; // nothing left to learn
    }
  }
  hint
}

const PUSH_DIR_ENTRIES_BATCH_COUNT: usize = 500;

struct ReadDirRunnerOptions {
  start_dir: PathBuf,
  config_discovery: ConfigDiscovery,
  thread_count: usize,
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
    while let Some(pending_dirs) = self.acquire_dirs() {
      let mut pending_count = 0;
      let mut all_entries = Vec::new();
      for current_dir in pending_dirs {
        match self.read_dir_entries(&current_dir) {
          Ok(Some(entries)) => {
            pending_count += entries.len();
            all_entries.push(DirEntries { path: current_dir, entries });
            // it is much faster to batch these than to hit the lock every time
            if pending_count > PUSH_DIR_ENTRIES_BATCH_COUNT {
              self.push_entries(std::mem::take(&mut all_entries));
              pending_count = 0;
            }
          }
          Ok(None) => continue,
          Err(err) => {
            self.finish_with_error(err);
            return;
          }
        }
      }
      self.finish_reading(all_entries);
    }
  }

  /// Reads a single directory, returning its entries to be matched.
  ///
  /// `Ok(None)` means the directory contributed nothing and should be skipped
  /// (it was empty or couldn't be read for a non-fatal reason).
  fn read_dir_entries(&self, current_dir: &Path) -> Result<Option<Vec<DirOrConfigEntry>>> {
    let entries = match self.environment.dir_info(current_dir) {
      Ok(entries) => entries,
      Err(err) => {
        if is_system_volume_error(current_dir, &err) {
          return Ok(None);
        }
        if err.kind() == std::io::ErrorKind::PermissionDenied {
          log_warn!(self.environment, "WARNING: Ignoring directory. Permission denied: {}", current_dir.display());
          return Ok(None);
        }
        return Err(anyhow!("Error reading dir '{}': {:#}", current_dir.display(), err));
      }
    };
    if entries.is_empty() {
      return Ok(None);
    }
    let maybe_config_file = if self.options.config_discovery.traverse_descendants() && current_dir != self.options.start_dir {
      entries
        .iter()
        .filter_map(|e| match e {
          DirEntry::Directory(_) => None,
          DirEntry::File { name, path } => {
            if name.to_str().is_some_and(|name| POSSIBLE_CONFIG_FILE_NAMES.contains(&name)) {
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
      Ok(Some(vec![DirOrConfigEntry::Config(config_file.clone())]))
    } else {
      Ok(Some(
        entries
          .into_iter()
          .map(|e| match e {
            DirEntry::Directory(path) => DirOrConfigEntry::Dir(path),
            DirEntry::File { path, .. } => DirOrConfigEntry::File(path),
          })
          .collect::<Vec<_>>(),
      ))
    }
  }

  /// Waits for directories to read, returning a chunk of them or `None` once the
  /// walk is finished (or aborted via an error on another thread).
  fn acquire_dirs(&self) -> Option<Vec<PathBuf>> {
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    loop {
      if state.shutdown {
        return None;
      }
      if !state.pending_dirs.is_empty() {
        let take = read_dir_chunk_size(state.pending_dirs.len(), self.options.thread_count);
        let chunk = state.pending_dirs.drain(..take).collect::<Vec<_>>();
        state.reading_count += 1;
        return Some(chunk);
      }
      // nothing to read right now; wait for the matching thread to feed more
      // directories or to signal that the walk is complete
      cvar.wait(&mut state);
    }
  }

  fn push_entries(&self, entries: Vec<DirEntries>) {
    if entries.is_empty() {
      return;
    }
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.pending_entries.push(entries);
    cvar.notify_all();
  }

  /// Pushes any remaining entries and marks this reader as no longer reading.
  fn finish_reading(&self, entries: Vec<DirEntries>) {
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    if !entries.is_empty() {
      state.pending_entries.push(entries);
    }
    state.finish_reading();
    cvar.notify_all();
  }

  /// Aborts the whole walk: records the error (first one wins) and marks this
  /// reader as no longer reading. Any entries this reader had already read are
  /// dropped — the matching thread surfaces the error rather than a partial result.
  fn finish_with_error(&self, error: Error) {
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    if state.error.is_none() {
      state.error = Some(error);
    }
    state.shutdown = true;
    state.finish_reading();
    cvar.notify_all();
  }
}

/// Maximum number of directories a reader grabs per lock acquisition. Kept small
/// so work stays balanced across readers and the matching thread is fed steadily,
/// while still amortizing the lock over several directories.
const READ_DIR_CHUNK_SIZE: usize = 8;

/// Hands out a share of the pending directories to a reader, while keeping the
/// chunk small enough that the matching thread stays fed and the readers stay
/// balanced across a wide directory level. Note that a single very large
/// directory is still read by one thread (a `read_dir` isn't splittable), so
/// this balances across directories, not within one.
fn read_dir_chunk_size(pending_len: usize, thread_count: usize) -> usize {
  (pending_len / thread_count.max(1)).clamp(1, READ_DIR_CHUNK_SIZE)
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
  git_ignore_tree: Option<GitIgnoreTree<TEnvironment>>,
}

impl<TEnvironment: Environment> GlobMatchingProcessor<TEnvironment> {
  pub fn new(shared_state: Arc<SharedState>, glob_matcher: GlobMatcher, git_ignore_tree: Option<GitIgnoreTree<TEnvironment>>) -> Self {
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
            // reuse the directory listing we already have to avoid extra file system calls
            let dir_hint = dir_entries_hint(&dir.entries);
            let gitignore = self
              .git_ignore_tree
              .as_mut()
              .and_then(|t| t.get_resolved_git_ignore_for_dir_children(&dir.path, dir_hint));
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
    if pending_dirs.is_empty() {
      return; // nothing new to read; don't bother waking the readers
    }
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    state.pending_dirs.extend(pending_dirs);
    cvar.notify_all();
  }

  fn get_next_entries(&self) -> Result<Option<Vec<Vec<DirEntries>>>> {
    let (lock, cvar) = &self.shared_state.inner;
    let mut state = lock.lock();
    loop {
      if let Some(err) = state.error.take() {
        return Err(err);
      }
      if !state.pending_entries.is_empty() {
        return Ok(Some(std::mem::take(&mut state.pending_entries)));
      }
      // when no reader is currently reading and there's nothing left to read,
      // the walk is complete: tell the readers to stop and finish up
      if state.reading_count == 0 && state.pending_dirs.is_empty() {
        state.shutdown = true;
        cvar.notify_all();
        return Ok(None);
      }
      // wait to be notified by a reader thread
      cvar.wait(&mut state);
    }
  }
}

struct SharedStateInternal {
  /// Directories waiting to be read by the reader threads.
  pending_dirs: VecDeque<PathBuf>,
  /// Batches of read directory entries waiting to be matched.
  pending_entries: Vec<Vec<DirEntries>>,
  /// Number of reader threads currently reading directories.
  reading_count: usize,
  /// The first error encountered by a reader thread, if any.
  error: Option<Error>,
  /// Set once the walk is complete (or aborted) so reader threads stop.
  shutdown: bool,
}

impl SharedStateInternal {
  /// Records that a reader has stopped reading the chunk it acquired. Every
  /// acquired chunk decrements exactly once, so this should never underflow;
  /// guard it anyway because an underflow would silently hang the walk (the
  /// `reading_count == 0` termination check could never become true).
  fn finish_reading(&mut self) {
    self.reading_count = self.reading_count.checked_sub(1).expect("reading_count underflow");
  }
}

struct SharedState {
  inner: (Mutex<SharedStateInternal>, Condvar),
}

impl SharedState {
  pub fn new(initial_dir: PathBuf) -> Self {
    SharedState {
      inner: (
        Mutex::new(SharedStateInternal {
          pending_dirs: VecDeque::from([initial_dir]),
          pending_entries: Vec::new(),
          reading_count: 0,
          error: None,
          shutdown: false,
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
        no_gitignore: false,
      },
    )
    .unwrap();
    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    expected_matches.sort();
    assert_eq!(result, expected_matches);
  }

  #[tokio::test]
  async fn should_match_same_files_regardless_of_read_thread_count() {
    // build a wide + deep tree (with a gitignore at the root) so the work is
    // split across many readers and fed back in several waves, then assert the
    // matched set is identical whether globbing with a single reader or a pool
    // of readers. only ordering and speed should differ — if reordered
    // traversal ever changed which files matched (e.g. a gitignore resolution
    // order dependency) this would catch it.
    let mut builder = TestEnvironmentBuilder::new();
    builder.write_file("/.git/HEAD", "");
    builder.write_file("/.gitignore", "ignored\n");
    for i in 0..200 {
      builder.write_file(format!("/dir{}/a.txt", i), "");
      builder.write_file(format!("/dir{}/nested/deep/b.txt", i), "");
      // excluded by the root .gitignore
      builder.write_file(format!("/dir{}/ignored/c.txt", i), "");
    }
    let environment = builder.build();
    let root_dir = environment.canonicalize("/").unwrap();
    let run = |read_threads: &str| {
      environment.set_env_var("DPRINT_GLOB_READ_THREADS", Some(read_threads));
      let result = glob(
        &environment,
        GlobOptions {
          start_dir: PathBuf::from("/"),
          config_discovery: ConfigDiscovery::Default,
          file_patterns: GlobPatterns {
            arg_includes: None,
            config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir.clone())]),
            arg_excludes: None,
            config_excludes: Vec::new(),
          },
          pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
          no_gitignore: false,
        },
      )
      .unwrap();
      let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
      result.sort();
      result
    };

    let serial = run("1");
    let parallel = run("16");
    assert_eq!(serial, parallel);
    // sanity: matched a.txt and nested/deep/b.txt for each dir, and the
    // gitignored files were excluded
    assert_eq!(serial.len(), 400);
    assert!(serial.iter().all(|p| !p.contains("ignored")));
  }

  #[tokio::test]
  async fn should_respect_git_info_exclude() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/.git/info/exclude", "excluded.txt")
      .write_file("/included.txt", "")
      .write_file("/excluded.txt", "")
      .write_file("/sub/included.txt", "")
      .write_file("/sub/excluded.txt", "")
      .build();
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
        no_gitignore: false,
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/included.txt", "/sub/included.txt"]);
  }

  #[tokio::test]
  async fn should_respect_global_gitignore_when_opted_in() {
    let environment = TestEnvironmentBuilder::new()
      // a `.git` dir makes `/` the repository root, where the global excludes apply
      .write_file("/.git/HEAD", "")
      .write_file("/global_ignore", "globally_excluded.txt")
      .write_file("/included.txt", "")
      .write_file("/globally_excluded.txt", "")
      .write_file("/sub/included.txt", "")
      .write_file("/sub/globally_excluded.txt", "")
      .build();
    environment.set_env_var("DPRINT_GLOBAL_GITIGNORE", Some("1"));
    environment.set_global_gitignore_path("/global_ignore");
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
        no_gitignore: false,
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    // the global excludes apply at the repo root and to its descendants
    assert_eq!(result, vec!["/included.txt", "/sub/included.txt"]);
  }

  #[tokio::test]
  async fn should_ignore_global_gitignore_when_not_opted_in() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/.git/HEAD", "")
      .write_file("/global_ignore", "globally_excluded.txt")
      .write_file("/included.txt", "")
      .write_file("/globally_excluded.txt", "")
      .build();
    // note: env var not set, so the global excludes file is ignored
    environment.set_global_gitignore_path("/global_ignore");
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
        no_gitignore: false,
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/globally_excluded.txt", "/included.txt"]);
  }

  #[tokio::test]
  async fn no_gitignore_should_override_global_gitignore() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/.git/HEAD", "")
      .write_file("/global_ignore", "globally_excluded.txt")
      .write_file("/included.txt", "")
      .write_file("/globally_excluded.txt", "")
      .build();
    environment.set_env_var("DPRINT_GLOBAL_GITIGNORE", Some("1"));
    environment.set_global_gitignore_path("/global_ignore");
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
        // `--no-gitignore` disables all gitignore handling, including the global file
        no_gitignore: true,
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/globally_excluded.txt", "/included.txt"]);
  }

  #[tokio::test]
  async fn should_match_literal_file_paths_without_traversal() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/sub/file.txt", "")
      .write_file("/sub/other.txt", "")
      .build();
    // error any attempt at reading a directory in order to
    // prove that literal file paths don't cause a traversal
    environment.set_dir_info_error(std::io::Error::other("FAILURE"));
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: Some(vec![
            GlobPattern::new("./sub/file.txt".to_string(), root_dir.clone()),
            GlobPattern::new("./not_exists.txt".to_string(), root_dir.clone()),
          ]),
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir)]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
        no_gitignore: false,
      },
    )
    .unwrap();
    assert_eq!(result.file_paths, vec![PathBuf::from("/sub/file.txt")]);
    assert!(result.config_files.is_empty());
  }

  #[tokio::test]
  async fn literal_file_path_in_dir_with_config_file_resolves_config() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/sub/dprint.json", "{}")
      .write_file("/sub/file.txt", "")
      .write_file("/sub/nested/dprint.json", "{}")
      .write_file("/sub/nested/file.txt", "")
      .write_file("/file.txt", "")
      .build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: Some(vec![
            GlobPattern::new("./file.txt".to_string(), root_dir.clone()),
            GlobPattern::new("./sub/file.txt".to_string(), root_dir.clone()),
            GlobPattern::new("./sub/nested/file.txt".to_string(), root_dir.clone()),
          ]),
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir)]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
        no_gitignore: false,
      },
    )
    .unwrap();
    // only the file directly in the current scope is matched and both files
    // below the sub config resolve to the shallowest config file, which will
    // discover the nested one recursively when its scope resolves
    assert_eq!(result.file_paths, vec![PathBuf::from("/file.txt")]);
    assert_eq!(result.config_files, vec![PathBuf::from("/sub/dprint.json")]);
  }

  #[tokio::test]
  async fn literal_file_path_ignores_config_files_when_config_discovery_disabled() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/sub/dprint.json", "{}")
      .write_file("/sub/file.txt", "")
      .build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Disabled,
        file_patterns: GlobPatterns {
          arg_includes: Some(vec![GlobPattern::new("./sub/file.txt".to_string(), root_dir.clone())]),
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir)]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
        no_gitignore: false,
      },
    )
    .unwrap();
    assert_eq!(result.file_paths, vec![PathBuf::from("/sub/file.txt")]);
    assert!(result.config_files.is_empty());
  }

  #[tokio::test]
  async fn literal_file_path_in_excluded_dir_not_matched() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/ignored/file.txt", "")
      // the config file is not discovered either because the
      // directory it's in is excluded
      .write_file("/ignored/dprint.json", "{}")
      .build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: Some(vec![GlobPattern::new("./ignored/file.txt".to_string(), root_dir.clone())]),
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir.clone())]),
          arg_excludes: None,
          config_excludes: vec![GlobPattern::new("./ignored".to_string(), root_dir)],
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
        no_gitignore: false,
      },
    )
    .unwrap();
    assert!(result.file_paths.is_empty());
    assert!(result.config_files.is_empty());
  }

  #[tokio::test]
  async fn should_traverse_from_glob_pattern_base_dir_outside_start_dir() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/other/file.txt", "")
      .write_file("/sub/file.txt", "")
      .build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      GlobOptions {
        // this happens when running `dprint fmt "../other/**"` from /sub
        start_dir: PathBuf::from("/sub"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: Some(vec![GlobPattern::new("./other/**".to_string(), root_dir.clone())]),
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir)]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
        no_gitignore: false,
      },
    )
    .unwrap();
    assert_eq!(result.file_paths, vec![PathBuf::from("/other/file.txt")]);
  }

  #[tokio::test]
  async fn should_expand_literal_dir_path_to_match_contents() {
    let environment = TestEnvironmentBuilder::new()
      .write_file("/sub/file1.txt", "")
      .write_file("/sub/nested/file2.txt", "")
      .write_file("/other/file3.txt", "")
      .build();
    let root_dir = environment.canonicalize("/").unwrap();
    let result = glob(
      &environment,
      GlobOptions {
        start_dir: PathBuf::from("/"),
        config_discovery: ConfigDiscovery::Default,
        file_patterns: GlobPatterns {
          arg_includes: Some(vec![GlobPattern::new("./sub".to_string(), root_dir.clone())]),
          config_includes: Some(vec![GlobPattern::new("**/*.txt".to_string(), root_dir)]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
        no_gitignore: false,
      },
    )
    .unwrap();
    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/sub/file1.txt", "/sub/nested/file2.txt"]);
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
        no_gitignore: false,
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
        no_gitignore: false,
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
        no_gitignore: false,
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
        no_gitignore: false,
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
        no_gitignore: false,
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/test/a/b/b.json", "/test/test.json"]);
  }

  #[tokio::test]
  async fn should_be_case_sensitive() {
    // https://github.com/dprint/dprint/issues/1082
    let environment = TestEnvironmentBuilder::new()
      .write_file("/src/FooSamlService.java", "")
      .write_file("/src/FooMlService.java", "")
      .write_file("/src/Other.java", "")
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
            GlobPattern::new("**/*.java".to_string(), root_dir.clone()),
            GlobPattern::new("!**/*MlService.java".to_string(), root_dir),
          ]),
          arg_excludes: None,
          config_excludes: Vec::new(),
        },
        pattern_base: CanonicalizedPathBuf::new_for_testing("/"),
        no_gitignore: false,
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    // FooSamlService.java should NOT be excluded — the pattern is *MlService, not *mlservice
    assert_eq!(result, vec!["/src/FooSamlService.java", "/src/Other.java"]);
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
        no_gitignore: false,
      },
    )
    .unwrap();

    let mut result = result.file_paths.into_iter().map(|r| r.to_string_lossy().to_string()).collect::<Vec<_>>();
    result.sort();
    assert_eq!(result, vec!["/dir/b/b.txt"]);
  }
}

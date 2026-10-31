use anyhow::Result;
use deno_terminal::colors;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use crate::arg_parser::CliArgs;
use crate::arg_parser::ConfigDiscovery;
use crate::arg_parser::SubCommand;
use crate::environment::CanonicalizedPathBuf;
use crate::environment::Environment;
use crate::utils::AncestorPaths;
use crate::utils::PathSource;
use crate::utils::ResolvedFilePathWithTextRef;
use crate::utils::UntrustedDownloadError;
use crate::utils::resolve_url_or_file_path_to_file_with_cache;

pub static POSSIBLE_CONFIG_FILE_NAMES: [&str; 4] = ["dprint.json", "dprint.jsonc", ".dprint.json", ".dprint.jsonc"];
/// The directory a config file may be put in instead of the directory it
/// applies to in order to reduce clutter there (ex. `.config/dprint.json`).
///
/// A `.config` directory is treated as part of its parent directory: its
/// config file is found when searching the parent directory and applies to
/// the parent directory, though its anchored patterns (ex. `./src/**`) are
/// still relative to the `.config` directory. The exception is the home
/// directory's `.config`, which is where the system's configuration goes
/// (ex. `XDG_CONFIG_HOME`) and so is an ordinary directory.
pub const DOT_CONFIG_DIR_NAME: &str = ".config";
/// The config file names searched for within a `.config` directory.
pub static DOT_CONFIG_FILE_NAMES: [&str; 2] = ["dprint.json", "dprint.jsonc"];

#[derive(Debug, Clone)]
pub struct ResolvedConfigPathWithText {
  pub source: PathSource,
  pub is_first_download: bool,
  pub content: String,
  pub base_path: CanonicalizedPathBuf,
  /// The directory the config file's anchored patterns (ex. `./src/**`) are
  /// relative to when it's not the base path, which is the `.config`
  /// directory for a config file in one.
  pub anchored_patterns_base_path: Option<CanonicalizedPathBuf>,
  pub is_global_config: bool,
  /// The path a local config file was found at when that isn't its canonical
  /// path (see `get_config_file_found_path`), which is what identifies it.
  pub found_path: Option<PathBuf>,
}

impl ResolvedConfigPathWithText {
  /// Gets the path of a local config file that identifies it, which is the
  /// path it was found at (see `found_path`).
  pub fn file_path(&self) -> Option<&Path> {
    get_config_file_path(&self.source, self.found_path.as_deref())
  }

  /// Gets the config file as it's shown, which is what identifies it (see
  /// `file_path`).
  pub fn display(&self) -> String {
    get_config_file_display(&self.source, self.found_path.as_deref())
  }

  pub fn as_file_path_with_text_ref(&self) -> ResolvedFilePathWithTextRef<'_> {
    ResolvedFilePathWithTextRef {
      content: &self.content,
      source: &self.source,
    }
  }
}

/// The error for a config file that was found on another machine (ex. a
/// `dprint.json` that's a symlink to a UNC path), which the download policy
/// doesn't allow reading without asking the user because reading it contacts
/// the machine (see `DownloadPolicy::ensure_allows_path`).
#[derive(Debug)]
pub struct ConfigFileOnAnotherMachineError {
  /// Where the config file was found, which is what's read once it's allowed.
  /// It's in a `.config` directory when that directory is on another machine.
  pub config_path: PathBuf,
  /// The directory it was found for, which it applies to.
  pub dir: PathBuf,
  pub download: UntrustedDownloadError,
}

impl std::fmt::Display for ConfigFileOnAnotherMachineError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    self.download.fmt(f)
  }
}

impl std::error::Error for ConfigFileOnAnotherMachineError {}

pub async fn resolve_main_config_path_and_bytes<TEnvironment: Environment>(
  args: &CliArgs,
  environment: &TEnvironment,
) -> Result<Option<ResolvedConfigPathWithText>> {
  fn get_default_paths(args: &CliArgs, environment: &impl Environment) -> Result<Option<ResolvedConfigPathWithText>> {
    let start_search_dir = get_start_search_directory(args, environment)?;
    // a `.config` directory is searched as part of its parent directory
    let start_search_dir = match start_search_dir.parent() {
      Some(parent) if is_dot_config_dir(environment, start_search_dir.as_ref()) => parent,
      _ => start_search_dir,
    };
    let ancestor_paths = environment.download_policy().ancestor_paths(start_search_dir.as_ref());
    let maybe_config_file = get_config_file_in_dir(start_search_dir.as_ref(), environment, &ancestor_paths, start_search_dir.as_ref())?;

    if let Some((path, content)) = maybe_config_file {
      let anchored_patterns_base_path = get_found_config_file_anchored_patterns_dir(&start_search_dir, start_search_dir.as_ref(), &path);
      let canonical_path = environment.canonicalize(&path)?;
      Ok(Some(ResolvedConfigPathWithText {
        found_path: get_found_config_file_path(&start_search_dir, start_search_dir.as_ref(), &path, canonical_path.as_ref()),
        source: PathSource::new_local(canonical_path),
        is_first_download: false,
        content,
        base_path: start_search_dir,
        anchored_patterns_base_path,
        is_global_config: false,
      }))
    } else {
      get_default_config_file_in_ancestor_directories(environment, environment.cwd().as_ref())
    }
  }

  fn get_start_search_directory(args: &CliArgs, environment: &impl Environment) -> std::io::Result<CanonicalizedPathBuf> {
    if let SubCommand::StdInFmt(command) = &args.sub_command {
      // When formatting via stdin, resolve the config file based on the
      // file path provided to the command. This is done for people who
      // format files in their editor.
      if environment.is_absolute_path(&command.file_name_or_path)
        && let Some(parent) = PathBuf::from(&command.file_name_or_path).parent()
      {
        return environment.canonicalize(parent);
      }
    }

    Ok(environment.cwd())
  }

  let config_discovery = args.config_discovery(environment);
  if let Some(config) = &args.config {
    let cwd = environment.cwd();
    let resolved_file = resolve_url_or_file_path_to_file_with_cache(config, &PathSource::new_local(cwd.clone()), args.remote_cache_mode(), environment)
      .await?
      .into_text()?;
    // a local config file's patterns are relative to its directory, the same as
    // when it's discovered, but a remote or npm config file has no directory
    let dirs = match &resolved_file.source {
      PathSource::Local(source) => {
        // use the path as specified when it's a plain file path (see `resolve_local_config_file_dirs`)
        let is_file_path = !config.starts_with("~/") && !config.starts_with("file:");
        let specified_path = if is_file_path {
          cwd.as_ref().join(config)
        } else {
          source.path.as_ref().to_path_buf()
        };
        resolve_local_config_file_dirs(environment, &specified_path)?
      }
      PathSource::Remote(_) | PathSource::Npm(_) => LocalConfigFileDirs {
        base_path: cwd,
        anchored_patterns_base_path: None,
      },
    };
    Ok(Some(ResolvedConfigPathWithText {
      content: resolved_file.content,
      source: resolved_file.source,
      is_first_download: resolved_file.is_first_download,
      base_path: dirs.base_path,
      anchored_patterns_base_path: dirs.anchored_patterns_base_path,
      is_global_config: false,
      found_path: None,
    }))
  } else if matches!(config_discovery, ConfigDiscovery::Global) {
    resolve_global_config_path_or_error(environment).map(Some)
  } else if config_discovery.traverse_ancestors()
    && let Some(path) = get_default_paths(args, environment)?
  {
    Ok(Some(path))
  } else if matches!(config_discovery, ConfigDiscovery::Default)
    && args.plugins.is_empty()
    && let ResolveGlobalConfigPathResult::Found(path) = resolve_global_config_path_and_text_detail(environment)?
  {
    Ok(Some(*path))
  } else {
    Ok(None)
  }
}

fn resolve_global_config_path_or_error(environment: &impl Environment) -> Result<ResolvedConfigPathWithText> {
  match resolve_global_config_path_and_text_detail(environment)? {
    ResolveGlobalConfigPathResult::Found(resolved_config_path) => Ok(*resolved_config_path),
    ResolveGlobalConfigPathResult::NotFound => anyhow::bail!("Could not find global dprint.json file. Create one with `dprint init --global`"),
    ResolveGlobalConfigPathResult::FailedResolvingSystemDir(err) => Err(anyhow::Error::from(err).context(concat!(
      "Could not find system config directory. ",
      "Maybe specify the DPRINT_CONFIG_DIR environment ",
      "variable to say where to store the global dprint configuration file."
    ))),
  }
}

pub fn resolve_global_config_path_and_text(environment: &impl Environment) -> std::io::Result<Option<ResolvedConfigPathWithText>> {
  match resolve_global_config_path_and_text_detail(environment)? {
    ResolveGlobalConfigPathResult::Found(resolved_config_text) => Ok(Some(*resolved_config_text)),
    ResolveGlobalConfigPathResult::NotFound | ResolveGlobalConfigPathResult::FailedResolvingSystemDir { .. } => Ok(None),
  }
}

/// Gets the global config file based at the root directory of the provided
/// path so the path is within the resulting config's directory. The global
/// config file is otherwise based at the cwd, and a config file only formats
/// the files within its base directory.
pub fn resolve_global_config_path_and_text_at_path_root(environment: &impl Environment, path: &Path) -> std::io::Result<Option<ResolvedConfigPathWithText>> {
  let Some(config_file) = resolve_global_config_path_and_text(environment)? else {
    return Ok(None);
  };
  // ex. the drive root on Windows
  let root_dir = path.ancestors().last().unwrap_or(path);
  Ok(Some(ResolvedConfigPathWithText {
    base_path: environment.canonicalize(root_dir)?,
    ..config_file
  }))
}

enum ResolveGlobalConfigPathResult {
  Found(Box<ResolvedConfigPathWithText>),
  NotFound,
  FailedResolvingSystemDir(std::io::Error),
}

fn resolve_global_config_path_and_text_detail(environment: &impl Environment) -> std::io::Result<ResolveGlobalConfigPathResult> {
  let global_folder = match resolve_global_config_dir(environment) {
    Ok(dir) => dir,
    Err(err) => return Ok(ResolveGlobalConfigPathResult::FailedResolvingSystemDir(err)),
  };
  for name in ["dprint.jsonc", "dprint.json"] {
    let file_path = global_folder.join_panic_relative(name);
    if let Some(content) = environment.maybe_read_file(&file_path)? {
      return Ok(ResolveGlobalConfigPathResult::Found(Box::new(ResolvedConfigPathWithText {
        source: PathSource::new_local(file_path),
        is_first_download: false,
        content,
        base_path: environment.cwd(),
        anchored_patterns_base_path: None,
        is_global_config: true,
        found_path: None,
      })));
    }
  }
  Ok(ResolveGlobalConfigPathResult::NotFound)
}

pub fn resolve_global_config_dir(environment: &impl Environment) -> std::io::Result<CanonicalizedPathBuf> {
  if let Some(folder) = resolve_env_var_folder(environment, "DPRINT_CONFIG_DIR")
    && let Ok(folder) = resolve_or_create_folder(environment, &folder).inspect_err(|err| {
      log_warn!(
        environment,
        "{} Could not resolve DPRINT_CONFIG_DIR value '{}'. Falling back to system configuration directory.",
        colors::yellow("Warning"),
        folder.display(),
      );
      log_debug!(environment, "Reason: {:#}", err);
    })
  {
    Ok(folder)
  } else {
    match resolve_system_config_dir(environment) {
      Some(dir) => resolve_or_create_folder(environment, dir.join("dprint")),
      None => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Could not find system config directory.")),
    }
  }
}

/// Gets where the global config directory is without creating anything,
/// which is for reading a file that may be in it. Unlike
/// [`resolve_global_config_dir`], a `DPRINT_CONFIG_DIR` that doesn't exist is
/// still where to look, since there's nothing to fall back from until
/// something is written there.
pub fn resolve_global_config_dir_path(environment: &impl Environment) -> Option<PathBuf> {
  match resolve_env_var_folder(environment, "DPRINT_CONFIG_DIR") {
    Some(folder) => Some(PathBuf::from(folder)),
    None => resolve_system_config_dir(environment).map(|dir| dir.join("dprint")),
  }
}

fn resolve_system_config_dir(environment: &impl Environment) -> Option<PathBuf> {
  if environment.os() == "macos" {
    resolve_mac_system_config_dir(environment)
  } else {
    environment.get_config_dir()
  }
}

/// For macOS, use XDG_CONFIG_HOME if available, otherwise check if the system config directory
/// exists, but fall back to $HOME/.config if it doesn't
fn resolve_mac_system_config_dir(environment: &impl Environment) -> Option<PathBuf> {
  // first, try XDG_CONFIG_HOME
  if let Some(xdg_config_home) = resolve_env_var_folder(environment, "XDG_CONFIG_HOME") {
    return Some(PathBuf::from(xdg_config_home));
  }

  // second, check if the system config directory exists
  if let Some(config_dir) = environment.get_config_dir() {
    // check if the dprint sub dir exists
    let dprint_config = config_dir.join("dprint");
    if environment.path_exists(&dprint_config) {
      return Some(config_dir);
    }
  }

  // fall back and prefer $HOME/.config
  if let Some(home_dir) = environment.get_home_dir() {
    return Some(home_dir.join(".config"));
  }

  None
}

fn resolve_env_var_folder(environment: &impl Environment, name: &str) -> Option<OsString> {
  environment.env_var(name).filter(|f| !f.is_empty())
}

fn resolve_or_create_folder(environment: &impl Environment, path: impl AsRef<Path>) -> std::io::Result<CanonicalizedPathBuf> {
  match environment.canonicalize(path.as_ref()) {
    Ok(path) => Ok(path),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
      if let (Some(parent), Some(filename)) = (path.as_ref().parent(), path.as_ref().file_name()) {
        _ = environment.mk_dir_all(parent);
        Ok(environment.canonicalize(parent)?.join_panic_relative(filename.to_string_lossy()))
      } else {
        Err(err)
      }
    }
    Err(err) => Err(err),
  }
}

/// Finds the config file in the directory or the closest ancestor directory.
///
/// IMPORTANT: It errors with a `ConfigFileOnAnotherMachineError` when the
/// download policy doesn't allow reading the config file it finds (or the
/// `.config` directory it's in) without asking the user, which is checked
/// before it's read because reading it contacts the machine. One on the same
/// share as the directory is read. It isn't skipped though, because that would
/// use another config file than the CLI would.
pub fn get_default_config_file_in_ancestor_directories(environment: &impl Environment, start_dir: &Path) -> Result<Option<ResolvedConfigPathWithText>> {
  let ancestor_paths = environment.download_policy().ancestor_paths(start_dir);
  // a `.config` directory is searched as part of its parent directory, which is next, and what's
  // above the root of a share isn't on it (ex. `/net/dprint.json` is a host named `dprint.json`)
  for ancestor_dir in ancestor_paths
    .ancestors(environment)
    .into_iter()
    .filter(|dir| !is_dot_config_dir(environment, dir))
  {
    if let Some((ancestor_config_path, content)) = get_config_file_in_dir(ancestor_dir, environment, &ancestor_paths, start_dir)? {
      let base_path = environment.canonicalize(ancestor_dir)?;
      let canonical_path = environment.canonicalize(&ancestor_config_path)?;
      return Ok(Some(ResolvedConfigPathWithText {
        found_path: get_found_config_file_path(&base_path, ancestor_dir, &ancestor_config_path, canonical_path.as_ref()),
        source: PathSource::new_local(canonical_path),
        is_first_download: false,
        content,
        anchored_patterns_base_path: get_found_config_file_anchored_patterns_dir(&base_path, ancestor_dir, &ancestor_config_path),
        base_path,
        is_global_config: false,
      }));
    }
  }

  Ok(None)
}

/// Errors with a `ConfigFileOnAnotherMachineError` when the download policy
/// doesn't allow reading the config file at a path that didn't come from
/// looking for it (ex. the uri of one that an editor sent) without asking the
/// user, like `get_default_config_file_in_ancestor_directories` does for one
/// it finds. One on the same share as its directory is allowed.
///
/// IMPORTANT: It's checked before the config file is read or canonicalized,
/// and before its directory is canonicalized (ex. to tell whether it's a
/// `.config` directory), because that contacts the machine.
pub fn ensure_allows_reading_config_file(environment: &impl Environment, config_path: &Path) -> Result<(), ConfigFileOnAnotherMachineError> {
  let Some(dir) = config_path.parent() else {
    return Ok(());
  };
  // the directory a config file in a `.config` directory applies to, which is only from
  // the names because telling whether it's the home directory's canonicalizes it
  let is_in_dot_config_dir = dir.file_name().is_some_and(|name| name == DOT_CONFIG_DIR_NAME)
    && config_path
      .file_name()
      .and_then(|name| name.to_str())
      .is_some_and(|name| DOT_CONFIG_FILE_NAMES.contains(&name));
  let base_dir = match dir.parent() {
    Some(parent) if is_in_dot_config_dir => parent,
    _ => dir,
  };
  environment
    .download_policy()
    .ensure_allows_path(environment, config_path, Some(base_dir))
    .map_err(|download| ConfigFileOnAnotherMachineError {
      config_path: config_path.to_path_buf(),
      dir: base_dir.to_path_buf(),
      download,
    })
}

/// Gets if the directory is a `.config` directory, which is treated as part
/// of its parent directory (see [`DOT_CONFIG_DIR_NAME`]).
pub fn is_dot_config_dir(environment: &impl Environment, dir: &Path) -> bool {
  dir.file_name().is_some_and(|name| name == DOT_CONFIG_DIR_NAME) && dir.parent().is_some_and(|parent| !is_home_dir(environment, parent))
}

/// The directories of a local config file.
pub struct LocalConfigFileDirs {
  /// The directory the config file applies to.
  pub base_path: CanonicalizedPathBuf,
  /// The directory the config file's anchored patterns are relative to when
  /// it's not the base path (see [`ResolvedConfigPathWithText`]).
  pub anchored_patterns_base_path: Option<CanonicalizedPathBuf>,
}

/// Resolves the directories of a local config file from the path it was
/// found at or specified with.
///
/// This uses the path before it's canonicalized because whether the config
/// file is in a `.config` directory depends on where it was found, which the
/// canonicalized path doesn't say when the config file or `.config` directory
/// is a symlink (or on a case insensitive file system).
pub fn resolve_local_config_file_dirs(environment: &impl Environment, config_path: &Path) -> std::io::Result<LocalConfigFileDirs> {
  match get_dot_config_file_base_dir(environment, config_path) {
    Some(base_dir) => {
      let base_path = environment.canonicalize(base_dir)?;
      Ok(LocalConfigFileDirs {
        anchored_patterns_base_path: Some(base_path.join_panic_relative(DOT_CONFIG_DIR_NAME)),
        base_path,
      })
    }
    None => {
      let config_path = environment.canonicalize(config_path)?;
      Ok(LocalConfigFileDirs {
        base_path: config_path.parent().unwrap_or_else(|| config_path.clone()),
        anchored_patterns_base_path: None,
      })
    }
  }
}

/// Resolves the directories of a local config file at a path that didn't
/// come from looking for it (ex. the uri of one that an editor sent), with the
/// path it's at where the directory it's used for is canonicalized (see
/// `get_config_file_found_path`).
///
/// Unlike `resolve_local_config_file_dirs`, it's used for the directory it's
/// in (or the parent of its `.config` directory) rather than the directory of
/// its canonical path, like when it's found for a file in that directory.
pub fn resolve_config_file_dirs_at_path(environment: &impl Environment, config_path: &Path) -> std::io::Result<(LocalConfigFileDirs, PathBuf)> {
  let (Some(dir), Some(file_name)) = (config_path.parent(), config_path.file_name()) else {
    return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "The config file path has no directory."));
  };
  match get_dot_config_file_base_dir(environment, config_path) {
    Some(base_dir) => {
      let base_path = environment.canonicalize(base_dir)?;
      let dot_config_dir = base_path.join_panic_relative(DOT_CONFIG_DIR_NAME);
      let path = dot_config_dir.as_ref().join(file_name);
      let dirs = LocalConfigFileDirs {
        anchored_patterns_base_path: Some(dot_config_dir),
        base_path,
      };
      Ok((dirs, path))
    }
    None => {
      let base_path = environment.canonicalize(dir)?;
      let path = base_path.as_ref().join(file_name);
      let dirs = LocalConfigFileDirs {
        base_path,
        anchored_patterns_base_path: None,
      };
      Ok((dirs, path))
    }
  }
}

/// Gets the path a local config file was found at to identify it by when
/// that isn't its canonical path, which is when it's a symlink to another
/// file (ex. `~/proj/dprint.json -> ~/dotfiles/dprint.json`) or it's in a
/// `.config` directory that's a symlink. The path that's provided has the
/// directory it was found for canonicalized.
///
/// A user knows the config file by where it was found, which is also where
/// it's used, so two symlinks to the same file are two config files. It's
/// still resolved from its canonical path (ex. what it extends is relative to
/// that). A canonical name that's only different in case (ex. on a case
/// insensitive file system) isn't another file.
pub fn get_config_file_found_path(found_path: PathBuf, canonical_path: &Path) -> Option<PathBuf> {
  let file_name = |path: &Path| path.file_name().and_then(|name| name.to_str()).map(|name| name.to_ascii_lowercase());
  let is_canonical = found_path.parent() == canonical_path.parent() && file_name(&found_path).is_some_and(|name| Some(name) == file_name(canonical_path));
  (!is_canonical).then_some(found_path)
}

/// Gets the path of a local config file that identifies it (see
/// `get_config_file_found_path`).
pub fn get_config_file_path<'a>(source: &'a PathSource, found_path: Option<&'a Path>) -> Option<&'a Path> {
  found_path.or_else(|| source.maybe_local_path().map(|path| path.as_ref()))
}

/// Gets how a config file is shown, which is what identifies it (see
/// `get_config_file_path`).
pub fn get_config_file_display(source: &PathSource, found_path: Option<&Path>) -> String {
  match found_path {
    Some(path) => path.display().to_string(),
    None => source.display(),
  }
}

/// Gets the directory a config file in a `.config` directory applies to,
/// which is the parent of the `.config` directory.
pub fn get_dot_config_file_base_dir<'a>(environment: &impl Environment, config_path: &'a Path) -> Option<&'a Path> {
  let file_name = config_path.file_name()?.to_str()?;
  let dir = config_path.parent()?;
  if DOT_CONFIG_FILE_NAMES.contains(&file_name) && is_dot_config_dir(environment, dir) {
    dir.parent()
  } else {
    None
  }
}

/// Gets the paths a config file may be at within the `.config` directory of
/// the provided directory, which is empty when there's no `.config` directory.
///
/// Checking for the `.config` directory first means a directory without one,
/// which is most of them, only costs a single stat.
pub fn get_dot_config_file_paths(environment: &impl Environment, dir: &Path) -> Vec<PathBuf> {
  let dot_config_dir = dir.join(DOT_CONFIG_DIR_NAME);
  if environment.path_exists(&dot_config_dir) && is_dot_config_dir(environment, &dot_config_dir) {
    DOT_CONFIG_FILE_NAMES.iter().map(|name| dot_config_dir.join(name)).collect()
  } else {
    Vec::new()
  }
}

/// Gets the config file in the directory, which is in the directory or the
/// ancestors of the base, whose config file is found (see
/// `get_default_config_file_in_ancestor_directories`).
fn get_config_file_in_dir(dir: &Path, environment: &impl Environment, ancestor_paths: &AncestorPaths, base: &Path) -> Result<Option<(PathBuf, String)>> {
  // IMPORTANT: Each path is checked before it's looked up because that may contact another machine.
  // Checking it looks it up, so it's only read when that didn't find that it doesn't exist.
  let ensure_allows = |path: &Path, config_path: &Path| {
    ancestor_paths
      .ensure_allows_existing(environment, path, Some(base))
      .map_err(|download| ConfigFileOnAnotherMachineError {
        config_path: config_path.to_path_buf(),
        dir: dir.to_path_buf(),
        download,
      })
  };
  for file_name in &POSSIBLE_CONFIG_FILE_NAMES {
    let config_path = dir.join(file_name);
    if ensure_allows(&config_path, &config_path)?
      && let Some(text) = environment.maybe_read_file(&config_path)?
    {
      return Ok(Some((config_path, text)));
    }
  }
  let dot_config_dir = dir.join(DOT_CONFIG_DIR_NAME);
  if !ensure_allows(&dot_config_dir, &dot_config_dir.join(DOT_CONFIG_FILE_NAMES[0]))? {
    return Ok(None);
  }
  for config_path in get_dot_config_file_paths(environment, dir) {
    // check it's a file first because reading within a `.config` that's
    // not a directory errors on some platforms rather than not finding it
    if ensure_allows(&config_path, &config_path)?
      && environment.path_is_file(&config_path)
      && let Some(text) = environment.maybe_read_file(&config_path)?
    {
      return Ok(Some((config_path, text)));
    }
  }
  Ok(None)
}

/// Gets the directory the anchored patterns of a config file found in the
/// provided directory by [`get_config_file_in_dir`] are relative to, which is
/// its `.config` directory when that's where the config file was found.
/// Gets the path a config file that was found in the directory is known by
/// (see `get_config_file_found_path`).
fn get_found_config_file_path(base_path: &CanonicalizedPathBuf, dir: &Path, config_path: &Path, canonical_path: &Path) -> Option<PathBuf> {
  let relative_path = config_path.strip_prefix(dir).ok()?;
  get_config_file_found_path(base_path.as_ref().join(relative_path), canonical_path)
}

fn get_found_config_file_anchored_patterns_dir(base_path: &CanonicalizedPathBuf, dir: &Path, config_path: &Path) -> Option<CanonicalizedPathBuf> {
  (config_path.parent() != Some(dir)).then(|| base_path.join_panic_relative(DOT_CONFIG_DIR_NAME))
}

fn is_home_dir(environment: &impl Environment, dir: &Path) -> bool {
  let Some(home_dir) = environment.get_home_dir() else {
    return false;
  };
  // the directory might not be canonicalized (ex. from a uri in the lsp), but
  // only canonicalize when necessary since this is called for every `.config`
  home_dir.as_ref() == dir
    // IMPORTANT: Canonicalizing follows symlinks, which contacts another machine (ex. a directory from
    // the editor that's a symlink to a share), so one that may not be read isn't the home directory.
    || (environment.download_policy().ensure_allows_path(environment, dir, dir.parent()).is_ok()
      && environment.canonicalize(dir).is_ok_and(|dir| dir == home_dir))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::environment::TestEnvironment;

  #[test]
  fn should_not_canonicalize_a_directory_on_another_machine_to_tell_whether_it_is_the_home_dir() {
    let env = TestEnvironment::new();
    env.set_download_policy(crate::utils::DownloadPolicy::for_editor(&env));
    env.mk_dir_all("/project").unwrap();
    let network_dir = if cfg!(windows) { r"\\server\share\dir" } else { "/net/server/share/dir" };
    env.symlink(network_dir, "/project/link").unwrap();
    // its `.config` directory is an ordinary one, which isn't read to tell
    assert!(is_dot_config_dir(&env, Path::new("/project/link/.config")));
    assert_eq!(env.canonicalize_count("/project/link"), 0);
    // while a local directory is canonicalized
    assert!(is_dot_config_dir(&env, Path::new("/project/.config")));
    assert_eq!(env.canonicalize_count("/project"), 1);
  }

  #[test]
  fn test_resolve_system_config_dir_macos_with_xdg_config_home() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    env.set_env_var("XDG_CONFIG_HOME", Some("/custom/xdg/config"));

    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/custom/xdg/config")));
  }

  #[test]
  fn test_resolve_system_config_dir_macos_with_empty_xdg_config_home() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    env.set_env_var("XDG_CONFIG_HOME", Some(""));

    // Empty XDG_CONFIG_HOME should be ignored, fall back to $HOME/.config
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/home/.config")));
  }

  #[test]
  fn test_resolve_system_config_dir_macos_with_existing_dprint_dir() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    env.mk_dir_all("/config/dprint").unwrap();

    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/config")));
  }

  #[test]
  fn test_resolve_system_config_dir_macos_without_existing_dprint_dir() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    // Don't create /config/dprint

    // Should fall back to $HOME/.config
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/home/.config")));
  }

  #[test]
  fn test_resolve_system_config_dir_macos_priority_xdg_over_existing() {
    let env = TestEnvironment::new();
    env.set_os("macos");
    env.set_env_var("XDG_CONFIG_HOME", Some("/custom/xdg"));
    env.mk_dir_all("/config/dprint").unwrap();

    // XDG_CONFIG_HOME should take priority even if dprint dir exists
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/custom/xdg")));
  }

  #[test]
  fn test_resolve_system_config_dir_linux() {
    let env = TestEnvironment::new();
    env.set_os("linux");

    // Non-macOS should use config_dir
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/config")));
  }

  #[test]
  fn test_resolve_system_config_dir_windows() {
    let env = TestEnvironment::new();
    env.set_os("windows");

    // Non-macOS should use config_dir
    let result = resolve_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/config")));
  }

  #[test]
  fn test_resolve_mac_system_config_dir_xdg_priority() {
    let env = TestEnvironment::new();
    env.set_env_var("XDG_CONFIG_HOME", Some("/xdg"));
    env.mk_dir_all("/config/dprint").unwrap();

    let result = resolve_mac_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/xdg")));
  }

  #[test]
  fn test_resolve_mac_system_config_dir_existing_dprint_priority() {
    let env = TestEnvironment::new();
    env.mk_dir_all("/config/dprint").unwrap();

    let result = resolve_mac_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/config")));
  }

  #[test]
  fn test_resolve_mac_system_config_dir_home_fallback() {
    let env = TestEnvironment::new();
    // No XDG_CONFIG_HOME, no existing dprint dir

    let result = resolve_mac_system_config_dir(&env);
    assert_eq!(result, Some(PathBuf::from("/home/.config")));
  }

  #[test]
  fn test_resolve_env_var_folder_with_value() {
    let env = TestEnvironment::new();
    env.set_env_var("TEST_VAR", Some("/some/path"));

    let result = resolve_env_var_folder(&env, "TEST_VAR");
    assert_eq!(result, Some(OsString::from("/some/path")));
  }

  #[test]
  fn test_resolve_env_var_folder_with_empty_value() {
    let env = TestEnvironment::new();
    env.set_env_var("TEST_VAR", Some(""));

    let result = resolve_env_var_folder(&env, "TEST_VAR");
    assert_eq!(result, None);
  }

  #[test]
  fn test_resolve_env_var_folder_not_set() {
    let env = TestEnvironment::new();

    let result = resolve_env_var_folder(&env, "NONEXISTENT_VAR");
    assert_eq!(result, None);
  }

  #[test]
  fn should_only_read_the_config_files_the_editors_found_when_checking_them() {
    let env = TestEnvironment::new();
    env.mk_dir_all("/repo/a/b").unwrap();
    env.write_file("/repo/dprint.json", "{}").unwrap();
    env.set_download_policy(crate::utils::DownloadPolicy::for_editor(&env));
    let config = get_default_config_file_in_ancestor_directories(&env, Path::new("/repo/a/b")).unwrap().unwrap();
    assert_eq!(config.base_path, CanonicalizedPathBuf::new_for_testing("/repo"));
    // checking that a path isn't on another machine looks it up, so one that doesn't exist isn't read
    for dir in ["/repo/a/b", "/repo/a"] {
      for file_name in POSSIBLE_CONFIG_FILE_NAMES {
        assert_eq!(env.file_read_count(Path::new(dir).join(file_name)), 0, "{} {}", dir, file_name);
      }
    }
    assert_eq!(env.file_read_count("/repo/dprint.json"), 1);
  }

  #[cfg(windows)]
  #[test]
  fn should_need_reading_a_config_file_found_on_another_machine_allowed_by_the_editors() {
    let env = TestEnvironment::new();
    env.mk_dir_all("/repo/src/sub").unwrap();
    env.write_file("/repo/dprint.json", "{}").unwrap();
    env.symlink(r"\\server\share\dprint.json", "/repo/src/dprint.json").unwrap();
    env.symlink(r"\\server\share\config", "/repo/src/sub/.config").unwrap();
    let find = |dir: &str| get_default_config_file_in_ancestor_directories(&env, Path::new(dir));
    // the test environment can't read what's on another machine, so this is what's asked about
    let find_asked = |dir: &str| {
      let err = find(dir).err()?;
      let err = err.downcast_ref::<ConfigFileOnAnotherMachineError>()?;
      Some((err.config_path.clone(), err.dir.clone(), err.download.download.clone()))
    };
    // the cli reads it
    assert_eq!(find_asked("/repo/src"), None);

    // the editors ask before contacting the machine, and don't use the config file of an ancestor directory instead
    env.set_download_policy(crate::utils::DownloadPolicy::for_editor(&env));
    let config_path = Path::new("/repo/src").join("dprint.json");
    let (path, dir, download) = find_asked("/repo/src").unwrap();
    assert_eq!(path, config_path);
    assert_eq!(dir, Path::new("/repo/src"));
    assert_eq!(download, format!(r"{} -> \\server\share\dprint.json", config_path.display()));
    // which includes a `.config` directory on another machine
    let dot_config_dir = Path::new("/repo/src/sub").join(".config");
    let (path, dir, dot_config_download) = find_asked("/repo/src/sub").unwrap();
    assert_eq!(path, dot_config_dir.join("dprint.json"));
    assert_eq!(dir, Path::new("/repo/src/sub"));
    assert_eq!(dot_config_download, format!(r"{} -> \\server\share\config", dot_config_dir.display()));
    // and it's read once it's allowed
    env.download_policy().allow(&[download]);
    assert_eq!(find_asked("/repo/src"), None);

    // a config file that's local is found
    env.mk_dir_all("/repo/other").unwrap();
    let config = find("/repo/other").unwrap().unwrap();
    assert_eq!(config.base_path, CanonicalizedPathBuf::new_for_testing("/repo"));
  }
}

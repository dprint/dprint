use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::path::Prefix;
use std::sync::Arc;

use parking_lot::Mutex;
use unicode_general_category::GeneralCategory;
use unicode_general_category::get_general_category;
use url::Url;

use crate::configuration::ConfigFileOnAnotherMachineError;
use crate::configuration::ResolveConfigError;
use crate::configuration::read_settings;
use crate::environment::DownloadProxy;
use crate::environment::Environment;
use crate::environment::PathKind;
use crate::resolution::ResolvePluginsError;

/// The domains that the editors download plugins and configuration files from
/// without asking, which the `editor.trustedDomains` setting adds to. They're
/// not trusted when the editors don't use the settings (see `Settings`).
pub const DEFAULT_TRUSTED_DOMAINS: [&str; 4] = ["registry.npmjs.org", "plugins.dprint.dev", "github.com", "githubusercontent.com"];

/// What may be downloaded without asking the user, which the environment
/// enforces for every request it sends (see `Environment::set_download_policy`).
///
/// IMPORTANT: Opening a project in an editor isn't the same as choosing to
/// download what its configuration file says. Downloading from any domain would
/// let a configuration file send information to a server of its choosing (ex.
/// that the project was opened, from where, and anything in the url), so the
/// editors only download from the trusted domains without asking. The same
/// goes for reading a file on another machine (ex. a UNC path on Windows).
///
/// The same goes for sending downloads through a proxy that an `.npmrc` file
/// other than the user's own sets (see `NpmrcProxy`), even to a trusted domain.
#[derive(Debug, Clone, Default)]
pub enum DownloadPolicy {
  /// Anything may be downloaded (ex. for the CLI, which the user ran).
  #[default]
  All,
  /// Only what's on the trusted domains or what the user allowed may be downloaded.
  TrustedDomains(Arc<TrustedDownloads>),
}

impl DownloadPolicy {
  /// Gets the policy of the editors, which only download from the trusted domains.
  pub fn for_editor(environment: &impl Environment) -> Self {
    DownloadPolicy::TrustedDomains(Arc::new(TrustedDownloads {
      trusted_domains: Mutex::new(TrustedDomains::from_settings(environment)),
      allowed: Default::default(),
      denied: Default::default(),
      not_allowable: Default::default(),
    }))
  }

  /// Gets whether the url may be downloaded without asking the user.
  pub fn allows(&self, url: &Url) -> bool {
    match self {
      DownloadPolicy::All => true,
      DownloadPolicy::TrustedDomains(downloads) => downloads.trusted_domains.lock().is_trusted(url) || downloads.is_allowed(url.as_str()),
    }
  }

  /// Gets whether downloads may be sent through the proxy of an `.npmrc` file
  /// without asking the user.
  pub fn allows_npmrc_proxy(&self, proxy: &NpmrcProxy) -> bool {
    match self {
      DownloadPolicy::All => true,
      DownloadPolicy::TrustedDomains(downloads) => downloads.allowed.lock().contains(&proxy.to_download()),
    }
  }

  /// Errors when the url may not be downloaded without asking the user.
  pub fn ensure_allows(&self, url: &Url) -> Result<(), UntrustedDownloadError> {
    if self.allows(url) {
      Ok(())
    } else {
      Err(UntrustedDownloadError { download: url.to_string() })
    }
  }

  /// Errors when the npm packages of the registry may not be downloaded
  /// without asking the user, which they may be from a trusted domain or once
  /// the user allowed the registry (see `get_npm_registry_download`).
  pub fn ensure_allows_npm_registry(&self, registry_url: &Url) -> Result<(), UntrustedDownloadError> {
    let DownloadPolicy::TrustedDomains(downloads) = self else {
      return Ok(());
    };
    let download = get_npm_registry_download(registry_url);
    if downloads.trusted_domains.lock().is_trusted(registry_url) || downloads.allowed.lock().contains(&download) {
      Ok(())
    } else {
      Err(UntrustedDownloadError { download })
    }
  }

  /// Errors when the url may not be downloaded through the proxy without
  /// asking the user.
  pub fn ensure_allows_download(&self, url: &Url, proxy: DownloadProxy<'_>) -> Result<(), UntrustedDownloadError> {
    self.ensure_allows(url)?;
    match proxy {
      DownloadProxy::Npmrc { source, .. } => self.ensure_allows_npmrc_proxy(source),
      DownloadProxy::Environment | DownloadProxy::Url(_) | DownloadProxy::Direct => Ok(()),
    }
  }

  /// Errors when downloads may not be sent through the proxy of an `.npmrc`
  /// file without asking the user.
  pub fn ensure_allows_npmrc_proxy(&self, proxy: &NpmrcProxy) -> Result<(), UntrustedDownloadError> {
    if self.allows_npmrc_proxy(proxy) {
      Ok(())
    } else {
      Err(UntrustedDownloadError { download: proxy.to_download() })
    }
  }

  /// Errors when the path is on another machine and may not be read without
  /// asking the user, which is allowed when the file it's relative to is on the
  /// same network share (ex. a config file in a project on a share).
  ///
  /// It follows the path through the file system like the system does (ex.
  /// `..` after a symlink on macOS and Linux, or `..` applied to the text
  /// first on Windows), and the share of the file it's relative to too, but
  /// never looks up what's on another machine (see `resolve_network_share`)
  /// until reading it is allowed. Then it follows the path on that share
  /// too, so a symlink there that points at a third machine needs its own
  /// approval.
  ///
  /// IMPORTANT: What's shown and allowed is the path along with where it
  /// resolved to when that's another path (ex. `C:\project\tools\plugin.wasm ->
  /// \\server\share\plugin.wasm` for a symlink), so the user sees the machine
  /// that's contacted and is asked again when the symlink points elsewhere.
  /// Allowing a directory allows what's in it that resolves to what's in where
  /// it resolved to (ex. the files of an npm package in `node_modules`).
  pub fn ensure_allows_path(&self, environment: &impl Environment, path: &Path, base: Option<&Path>) -> Result<(), UntrustedDownloadError> {
    match self {
      DownloadPolicy::All => Ok(()),
      DownloadPolicy::TrustedDomains(downloads) => downloads.ensure_allows_path(environment, path, base, PATH_RULES),
    }
  }

  /// Gets what checks reading the paths in the directory and its ancestors
  /// (ex. their `.npmrc` files) like `ensure_allows_path`, which follows the
  /// directories through the file system once for all of them.
  pub fn ancestor_paths(&self, dir: &Path) -> AncestorPaths {
    AncestorPaths::new(self, dir, PATH_RULES)
  }

  /// Allows downloading what the user was shown and allowed (the downloads
  /// of `UntrustedDownloadError`s) for the rest of the session.
  pub fn allow(&self, downloads: &[String]) {
    if let DownloadPolicy::TrustedDomains(trusted_downloads) = self {
      trusted_downloads
        .allowed
        .lock()
        .extend(downloads.iter().map(|download| normalize_download(download)));
    }
  }

  /// Sets what the user denied downloading this session for any config file,
  /// which an allowed npm registry or directory above it then doesn't allow
  /// (see `TrustedDownloads::is_allowed` and `is_path_allowed`).
  pub fn set_denied_downloads(&self, downloads: impl IntoIterator<Item = String>) {
    if let DownloadPolicy::TrustedDomains(trusted_downloads) = self {
      *trusted_downloads.denied.lock() = downloads.into_iter().map(|download| normalize_download(&download)).collect();
    }
  }

  /// Gets whether allowing the download would allow it, which it doesn't for
  /// a path that isn't valid Unicode (see `TrustedDownloads::is_path_allowed`).
  pub fn is_allowable(&self, download: &str) -> bool {
    match self {
      DownloadPolicy::All => true,
      DownloadPolicy::TrustedDomains(trusted_downloads) => !trusted_downloads.not_allowable.lock().contains(download),
    }
  }

  /// Gets what the user allowed downloading this session.
  pub fn allowed_downloads(&self) -> Vec<String> {
    match self {
      DownloadPolicy::All => Vec::new(),
      DownloadPolicy::TrustedDomains(trusted_downloads) => trusted_downloads.allowed.lock().iter().cloned().collect(),
    }
  }

  /// Reads the trusted domains from the settings file again (ex. after it changed).
  pub fn reload_trusted_domains(&self, environment: &impl Environment) {
    if let DownloadPolicy::TrustedDomains(trusted_downloads) = self {
      *trusted_downloads.trusted_domains.lock() = TrustedDomains::from_settings(environment);
    }
  }
}

/// What may be downloaded with the `TrustedDomains` download policy.
#[derive(Debug)]
pub struct TrustedDownloads {
  trusted_domains: Mutex<TrustedDomains>,
  /// The urls and paths the user allowed downloading this session, which are only the
  /// exact ones they were shown, apart from an npm registry, which allows what's under
  /// it (see `get_npm_registry_download`). It also has the proxies of `.npmrc` files the
  /// user allowed downloading through (see `NpmrcProxy::to_download`).
  allowed: Mutex<HashSet<String>>,
  /// What the user denied downloading this session, which wins over an npm
  /// registry or a directory above it that allows what's under it, but not
  /// over allowing it exactly. A denied directory denies what's in it the
  /// same way, so the most specific decision is what's used.
  denied: Mutex<HashSet<String>>,
  /// The downloads of the paths that aren't valid Unicode that were asked
  /// about, which allowing doesn't allow (see `is_path_allowed`).
  not_allowable: Mutex<HashSet<String>>,
}

impl TrustedDownloads {
  fn is_allowed(&self, url: &str) -> bool {
    // IMPORTANT: Only a registry allows what's under it, because the user was told what's allowed
    // is what they're shown (ex. not every url of a domain for `https://example.com/`). The text of
    // each download is kept in session files and hashed into approval ids, which other versions read,
    // so what an existing form allows is never broadened, and a broader allow gets a new form instead
    // (see `SessionFile`).
    let allowed = self.allowed.lock();
    if allowed.contains(url) {
      return true;
    }
    // IMPORTANT: A denial wins over a registry that allows what's under it (ex. what another config file extends
    // from the registry's host, which the user denied for that config file).
    if self.denied.lock().contains(url) {
      return false;
    }
    allowed.iter().any(|allowed| {
      // IMPORTANT: Only a registry url that ends with a slash is a prefix, because one that doesn't (ex. from a
      // session file another version or a corrupted file wrote) would allow other hosts (ex. `https://npm.corp`
      // would allow `https://npm.corp.example`).
      get_npm_registry_download_url(allowed).is_some_and(|registry_url| registry_url.ends_with('/') && url.starts_with(registry_url))
    })
  }

  /// Errors when the path may not be read without asking the user (see
  /// `DownloadPolicy::ensure_allows_path`).
  fn ensure_allows_path(&self, environment: &impl Environment, path: &Path, base: Option<&Path>, rules: PathRules) -> Result<(), UntrustedDownloadError> {
    // the share of the file it's relative to is already being read
    let mut base_share = base.and_then(|base| get_network_share(base, rules.is_net_automounted));
    let mut network_path = resolve_network_share(environment, path, rules, base_share.clone());
    // the file it's relative to may be on a share that an automounter mounted, which is
    // only found through the file system, so that's only done when the path reached a share
    if network_path.is_some()
      && base_share.is_none()
      && let Some(automounted_share) = base.and_then(|base| get_automounted_share(environment, base, rules))
    {
      network_path = resolve_network_share(environment, path, rules, Some(automounted_share.clone()));
      base_share = Some(automounted_share);
    }
    self.ensure_allows_network_path(path, network_path, base_share.as_deref(), |allowed_shares| {
      resolve_network_share(environment, path, rules, base_share.iter().chain(allowed_shares).cloned())
    })
  }

  /// Errors when the path that's on another machine may not be read without
  /// asking the user (see `DownloadPolicy::ensure_allows_path`), which it may
  /// be when it's on the share of the file it's relative to.
  ///
  /// IMPORTANT: Once reading it is allowed, it's followed again with the
  /// symlinks on its share (`follow` with the shares that were allowed), so a
  /// symlink there can't reach a machine the user wasn't shown (ex. an NFS
  /// client on macOS follows a symlink to `/net/other` when it's read), which
  /// is then approved like any other. Looking up what's on the share doesn't
  /// contact anything new because its machine is allowed to be read.
  fn ensure_allows_network_path(
    &self,
    path: &Path,
    mut network_path: Option<NetworkPath>,
    base_share: Option<&str>,
    follow: impl Fn(&[String]) -> Option<NetworkPath>,
  ) -> Result<(), UntrustedDownloadError> {
    let mut allowed_shares = Vec::new();
    loop {
      let Some(current) = network_path else {
        return Ok(());
      };
      if base_share == Some(current.share.as_str()) || allowed_shares.contains(&current.share) {
        return Ok(());
      }
      // each share is only allowed once, but a share's symlinks may point elsewhere each time they're read
      if allowed_shares.len() >= MAX_SYMLINKS || !self.is_path_allowed(path, &current.resolved) {
        let download = get_path_download(path, &current.resolved);
        if path.to_str().is_none() || current.resolved.to_str().is_none() {
          self.not_allowable.lock().insert(download.clone());
        }
        return Err(UntrustedDownloadError { download });
      }
      allowed_shares.push(current.share);
      network_path = follow(&allowed_shares);
    }
  }

  /// Gets whether reading the path on another machine that resolved to the
  /// other path was allowed, which it also is when a directory it's in was
  /// allowed and the rest of it is the same as the rest of what it resolved to.
  ///
  /// IMPORTANT: Only what the user was shown is allowed, not what starts with
  /// it like for an npm registry (see `get_npm_registry_download`), because
  /// that may leave it (ex. `/net/host/share/../../other/plugin.wasm` reaches
  /// another machine). What a path allows is kept in session files and its
  /// text is hashed into approval ids, which other versions read, so it's
  /// never broadened (see `SessionFile`).
  ///
  /// IMPORTANT: A path that isn't valid Unicode is never allowed, because its
  /// text has a replacement character where it isn't (see `Path::display`), so
  /// another path would have the same text. The user is asked each time.
  fn is_path_allowed(&self, path: &Path, resolved: &Path) -> bool {
    if path.to_str().is_none() || resolved.to_str().is_none() {
      return false;
    }
    let (mut path, mut resolved) = (path, resolved);
    loop {
      let download = get_path_download(path, resolved);
      if self.allowed.lock().contains(&download) {
        return true;
      }
      // IMPORTANT: A denial of the path or of a directory it's in wins over allowing a directory above that (ex. the
      // npm package of another config file), so the most specific decision is used, which is an allow when the same
      // path is both allowed and denied (ex. allowing it exactly for another config file).
      if self.denied.lock().contains(&download) {
        return false;
      }
      match (path.file_name(), resolved.file_name(), path.parent(), resolved.parent()) {
        (Some(name), Some(resolved_name), Some(parent), Some(resolved_parent)) if name == resolved_name => {
          path = parent;
          resolved = resolved_parent;
        }
        _ => return false,
      }
    }
  }
}

/// A proxy that an `.npmrc` file other than the user's own (`~/.npmrc`) sets,
/// such as one in the project or one of its ancestor directories.
///
/// IMPORTANT: The editors only send downloads through it once the user allows
/// it. The project would otherwise choose a server that's sent every download
/// along with the credentials in the setting, which may have the values of the
/// user's environment variables (ex. `https://${GITHUB_TOKEN}@proxy.example`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NpmrcProxy {
  /// The path of the `.npmrc` file.
  pub npmrc_path: String,
  /// The text of the setting as it is in the file, before its environment
  /// variables are replaced, which is what the user is shown.
  pub setting: String,
}

impl NpmrcProxy {
  /// Gets what the user allows to download through the proxy, which is one of
  /// the downloads of an `UntrustedDownloadError` like a url. It's the file
  /// with the text of its setting, so a changed setting needs to be allowed
  /// again.
  pub fn to_download(&self) -> String {
    // the setting is a line of the file, so it can't have a newline
    format!("{}{}\n{}", NPMRC_PROXY_DOWNLOAD_PREFIX, self.setting, self.npmrc_path)
  }

  /// Gets the proxy of a download that's from `to_download`, which is `None`
  /// for a url.
  pub fn from_download(download: &str) -> Option<Self> {
    let (setting, npmrc_path) = download.strip_prefix(NPMRC_PROXY_DOWNLOAD_PREFIX)?.split_once('\n')?;
    Some(NpmrcProxy {
      npmrc_path: npmrc_path.to_string(),
      setting: setting.to_string(),
    })
  }
}

/// Checks reading the paths in a directory and its ancestors (ex. their
/// `.npmrc` files or the packages in their `node_modules`) like
/// `DownloadPolicy::ensure_allows_path`.
///
/// It follows the directories through the file system once, the first time a
/// path is checked, and then only follows what's after the directory a path
/// is in, instead of following each path from the root. The share the
/// directory is on is followed like a local path (see `PathWalk`), because
/// its machine is already being read.
pub struct AncestorPaths {
  /// `None` when anything may be read (see `DownloadPolicy::All`).
  downloads: Option<Arc<TrustedDownloads>>,
  dir: PathBuf,
  rules: PathRules,
  /// The share the directory is on, with how the directory and each of its
  /// ancestors were followed.
  walks: OnceCell<(Option<String>, HashMap<PathBuf, PathWalk>)>,
}

impl AncestorPaths {
  fn new(policy: &DownloadPolicy, dir: &Path, rules: PathRules) -> Self {
    AncestorPaths {
      downloads: match policy {
        DownloadPolicy::All => None,
        DownloadPolicy::TrustedDomains(downloads) => Some(downloads.clone()),
      },
      dir: dir.to_path_buf(),
      rules,
      walks: OnceCell::new(),
    }
  }

  /// Gets the directory and its ancestors, which stop at the root of the
  /// network share the directory is on (see `parent_on_share`).
  pub fn ancestors(&self, environment: &impl Environment) -> Vec<&Path> {
    std::iter::successors(Some(self.dir.as_path()), |dir| self.parent_on_share(environment, dir)).collect()
  }

  /// Gets the parent of the directory or one of its ancestors, which is
  /// `None` for the root of the network share the directory is on (ex.
  /// `/net/host` on macOS) because what's above it isn't on the share. For
  /// example, `/net/.npmrc` isn't a file but a host named `.npmrc`, which the
  /// automounter would look up. The same goes for the root of a share that an
  /// automounter mounted elsewhere (ex. `/home/me` when `/home` is an
  /// `autofs` directory).
  ///
  /// When anything may be read (ex. the CLI), it only stops at a share in
  /// `/net` on Linux when `/net` is an automounter's, so a project in an
  /// ordinary `/net` directory still finds what's above it.
  pub fn parent_on_share<'a>(&self, environment: &impl Environment, dir: &'a Path) -> Option<&'a Path> {
    let Some(parent) = get_parent_on_share_with(dir, self.rules.is_net_automounted) else {
      // the root of a share in `/net`, or a path without a parent
      return match dir.parent() {
        Some(parent) if self.downloads.is_none() && !self.rules.is_net_always_automounted && !environment.is_automount_dir(parent) => Some(parent),
        _ => None,
      };
    };
    if self.downloads.is_none() {
      return Some(parent);
    }
    let (followed_share, walks) = self.get_walks(environment);
    let Some(followed_share) = followed_share else {
      return Some(parent);
    };
    let share = PathLocation::Share(followed_share.clone());
    let location = |dir: &Path| walks.get(&*self.rules.normalize(dir)).map(|walk| &walk.location);
    match (location(dir), location(parent)) {
      (Some(location), Some(parent_location)) if *location == share && *parent_location != share => None,
      _ => Some(parent),
    }
  }

  /// Errors when the path, which is in the directory or one of its ancestors,
  /// is on another machine and may not be read without asking the user (see
  /// `DownloadPolicy::ensure_allows_path`).
  pub fn ensure_allows(&self, environment: &impl Environment, path: &Path, base: Option<&Path>) -> Result<(), UntrustedDownloadError> {
    self.ensure_allows_existing(environment, path, base).map(|_| ())
  }

  /// Errors like `ensure_allows`, which also gets whether the path may exist.
  /// It's `false` when following it found that it doesn't, so it doesn't need
  /// to be read, and `true` otherwise (ex. when nothing was looked up because
  /// anything may be read).
  pub fn ensure_allows_existing(&self, environment: &impl Environment, path: &Path, base: Option<&Path>) -> Result<bool, UntrustedDownloadError> {
    let Some(downloads) = &self.downloads else {
      return Ok(true);
    };
    let (followed_share, walks) = self.get_walks(environment);
    // the walks are of the directories as they're followed (ex. without `..` on Windows)
    let followed_path = self.rules.normalize(path);
    let walk = match followed_path.ancestors().find_map(|dir| Some((dir, walks.get(dir)?))) {
      Some((dir, walk)) => {
        let mut walk = walk.clone();
        walk.follow(environment, followed_path.strip_prefix(dir).unwrap_or(path), self.rules, |_| {});
        walk
      }
      // ex. it's in a directory that's after a symlink loop
      None => {
        let mut walk = PathWalk::new(followed_share.clone());
        walk.follow(environment, path, self.rules, |_| {});
        walk
      }
    };
    let is_missing = walk.is_missing;
    let network_path = walk.into_network_path(path);
    let base_share = match &network_path {
      Some(_) => base.and_then(|base| self.get_base_share(environment, base)),
      None => None,
    };
    downloads.ensure_allows_network_path(path, network_path, base_share.as_deref(), |allowed_shares| {
      let mut walk = PathWalk::new(followed_share.iter().chain(allowed_shares).cloned());
      walk.follow(environment, path, self.rules, |_| {});
      walk.into_network_path(path)
    })?;
    Ok(!is_missing)
  }

  /// Gets the share the directory is on, which is followed like a local path,
  /// with how the directory and each of its ancestors were followed. The
  /// share is from the text of the directory, or else from following it when
  /// an automounter mounted it (see `get_automounted_share`).
  fn get_walks(&self, environment: &impl Environment) -> (&Option<String>, &HashMap<PathBuf, PathWalk>) {
    let (share, walks) = self.walks.get_or_init(|| {
      let share = get_network_share(&self.dir, self.rules.is_net_automounted);
      let walks = PathWalk::follow_ancestors(environment, &self.dir, self.rules, share.clone());
      if share.is_some() {
        return (share, walks);
      }
      match walks.get(&*self.rules.normalize(&self.dir)).and_then(|walk| walk.get_automounted_share()) {
        Some(share) => {
          let walks = PathWalk::follow_ancestors(environment, &self.dir, self.rules, Some(share.clone()));
          (Some(share), walks)
        }
        None => (None, walks),
      }
    });
    (share, walks)
  }

  /// Gets the share of the file a path is relative to like
  /// `TrustedDownloads::ensure_allows_path`, which is already known for the
  /// directory and its ancestors.
  fn get_base_share(&self, environment: &impl Environment, base: &Path) -> Option<String> {
    if let Some(share) = get_network_share(base, self.rules.is_net_automounted) {
      return Some(share);
    }
    let (followed_share, walks) = self.get_walks(environment);
    match walks.get(&*self.rules.normalize(base)) {
      Some(walk) => followed_share.clone().filter(|share| walk.location == PathLocation::Share(share.clone())),
      None => get_automounted_share(environment, base, self.rules),
    }
  }
}

/// The error for something that needs to be downloaded from where the
/// download policy doesn't allow without asking the user.
#[derive(Debug)]
pub struct UntrustedDownloadError {
  /// The url, or the proxy of an `.npmrc` file (see `NpmrcProxy::to_download`).
  pub download: String,
}

impl std::fmt::Display for UntrustedDownloadError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if let Some(proxy) = NpmrcProxy::from_download(&self.download) {
      write!(
        f,
        "Downloading through the proxy {} of {} needs to be allowed because the .npmrc file isn't the user's own.",
        quote_text(&proxy.setting),
        quote_text(&proxy.npmrc_path)
      )
    } else if is_path_download(&self.download) {
      write!(
        f,
        "Reading {} needs to be allowed because it's on another machine.",
        quote_download(&self.download)
      )
    } else {
      write!(
        f,
        "Downloading {} needs to be allowed because its domain isn't trusted.",
        quote_download(&self.download)
      )
    }
  }
}

impl std::error::Error for UntrustedDownloadError {}

impl UntrustedDownloadError {
  /// Finds the error in the chain of an error.
  pub fn find(err: &anyhow::Error) -> Option<&UntrustedDownloadError> {
    err.chain().find_map(|err| {
      // a transparent error's source is the source of what it wraps
      if let Some(ResolveConfigError::Other(inner)) = err.downcast_ref::<ResolveConfigError>() {
        Self::find(inner)
      } else if let Some(ResolvePluginsError(inner)) = err.downcast_ref::<ResolvePluginsError>() {
        Self::find(inner)
      } else if let Some(err) = err.downcast_ref::<ConfigFileOnAnotherMachineError>() {
        Some(&err.download)
      } else {
        err.downcast_ref::<UntrustedDownloadError>()
      }
    })
  }
}

/// Gets what the user allows to download npm packages from a registry, which
/// is one of the downloads of an `UntrustedDownloadError` like a url. Unlike a
/// url, which only allows itself, it allows each url under the registry's (ex.
/// each package and its tarball), which ends with a slash so it's only what's
/// under its path.
pub fn get_npm_registry_download(registry_url: &Url) -> String {
  let mut url = registry_url.to_string();
  if !url.ends_with('/') {
    url.push('/');
  }
  format!("{}{}", NPM_REGISTRY_DOWNLOAD_PREFIX, url)
}

/// Gets the url of the npm registry of a download from
/// `get_npm_registry_download`, which is `None` for another download.
pub fn get_npm_registry_download_url(download: &str) -> Option<&str> {
  download.strip_prefix(NPM_REGISTRY_DOWNLOAD_PREFIX)
}

/// Gets what's shown of a download that's a url (ex. the url of an npm
/// registry), which is the download itself for another one.
pub fn get_download_url_text(download: &str) -> &str {
  get_npm_registry_download_url(download).unwrap_or(download)
}

/// Gets whether a download is reading a path on another machine (see
/// `DownloadPolicy::ensure_allows_path`), which is what isn't a url or the
/// proxy of an `.npmrc` file.
///
/// A path is never mistaken for a url because it's absolute, so it starts with
/// a slash, a backslash, or a drive letter, which is a scheme of one letter.
pub fn is_path_download(download: &str) -> bool {
  NpmrcProxy::from_download(download).is_none() && Url::parse(download).map_or(true, |url| url.scheme().len() == 1)
}

/// Gets the text of a download as this version makes it, which is what it's
/// compared by. A path download's text is made again from the paths it's of,
/// because how a path is escaped depends on the version of dprint (ex. the
/// version of Unicode that tells which characters aren't assigned), so a text
/// that another version wrote (ex. in a session file) means the same paths.
pub fn normalize_download(download: &str) -> String {
  if !is_path_download(download) {
    return download.to_string();
  }
  // an arrow in a path is escaped, so the first one is the one between them
  match download.split_once(" -> ") {
    Some((path, resolved)) => get_path_download(Path::new(&unescape_text(path)), Path::new(&unescape_text(resolved))),
    None => {
      let path = unescape_text(download);
      get_path_download(Path::new(&path), Path::new(&path))
    }
  }
}

/// Gets the path in the project of a download that reads a path on another
/// machine through a symlink (the path before ` -> `), which is `None` when
/// the path itself is on another machine.
pub fn get_path_download_local_path(download: &str) -> Option<PathBuf> {
  // an arrow in a path is escaped, so the first one is the one between them
  let (path, _) = download.split_once(" -> ")?;
  Some(PathBuf::from(unescape_text(path)))
}

/// The titles of the actions of the message that asks whether to download from
/// domains that aren't trusted.
pub const DOWNLOAD_ACTION_TITLE: &str = "Download";
pub const CANCEL_DOWNLOAD_ACTION_TITLE: &str = "Cancel";

/// Gets the message that asks the user whether to download what a config file
/// needs from domains that aren't trusted, through the proxies of `.npmrc`
/// files that aren't the user's own, or from another machine.
///
/// Each proxy is said first, starting with its setting and its file, because
/// an editor may cut off a long message (ex. after many long urls) and the
/// user needs to see what's sent to it. The paths are next because the
/// settings file can't trust them.
///
/// The settings file isn't said to trust a domain when the editors don't use
/// it (see `read_ignored_settings_message`), which is said instead.
pub fn get_untrusted_download_message(downloads: &[String], ignored_settings_message: Option<&str>) -> String {
  let (proxies, others): (Vec<_>, Vec<_>) = downloads.iter().partition(|download| NpmrcProxy::from_download(download).is_some());
  let (paths, urls): (Vec<_>, Vec<_>) = others.into_iter().partition(|download| is_path_download(download));
  let mut messages = Vec::new();
  for proxy in proxies.iter().filter_map(|download| NpmrcProxy::from_download(download)) {
    messages.push(format!(
      concat!(
        "The proxy {} that {} sets isn't trusted because it isn't your own .npmrc file. ",
        "Only allow dprint to download through it when you trust the .npmrc file because the proxy receives every download ",
        "and the credentials in the setting, including the values of the environment variables it has."
      ),
      quote_text(&proxy.setting),
      quote_text(&proxy.npmrc_path),
    ));
  }
  if !paths.is_empty() {
    messages.push(format!(
      concat!(
        "dprint needs to read {}, which {} on another machine. ",
        "Only allow it when you trust the configuration file because reading from the machine connects to it, ",
        "which can send it information (ex. that the project was opened, or your credentials for the machine)."
      ),
      paths.iter().map(|path| quote_download(path)).collect::<Vec<_>>().join(", "),
      if paths.len() == 1 { "is" } else { "are" },
    ));
    if paths.iter().any(|path| is_npmrc_path_download(path)) {
      messages.push(
        "An .npmrc file sets where npm packages are downloaded from (ex. a company's registry for a scope), so npm packages aren't resolved until it's read."
          .to_string(),
      );
    }
  }
  if !urls.is_empty() {
    // said first because an editor may cut off a long message, and allowing a registry allows more than what's shown
    let registries = urls.iter().filter(|url| get_npm_registry_download_url(url).is_some()).collect::<Vec<_>>();
    if !registries.is_empty() {
      messages.push(format!(
        "{} {} an npm registry, so allowing it allows every url under it (ex. each package from it).",
        registries.iter().map(|url| quote_download(url)).collect::<Vec<_>>().join(", "),
        if registries.len() == 1 { "is" } else { "are each" },
      ));
    }
    messages.push(format!(
      concat!(
        "dprint needs to download {} from domains that aren't trusted. ",
        "Only download them when you trust the configuration file because a download can send information to the domain. {}"
      ),
      urls.iter().map(|url| quote_download(url)).collect::<Vec<_>>().join(", "),
      ignored_settings_message.unwrap_or("Add a domain to the editor.trustedDomains setting in dprint's settings file to trust it."),
    ));
  }
  messages.join(" ")
}

/// Quotes text from the project (ex. a url, a path, or the setting of an
/// `.npmrc` file) in a message with backticks (ex. `` `https://example.com` ``),
/// so it's clear where it starts and ends and it can't read as part of the
/// message's sentences.
///
/// IMPORTANT: The editors show the message as plain text, so the text can't
/// have anything that would end the quotes or change how the message reads. A
/// backtick and what looks like one are shown as an escape (ex. `\u{60}`) along
/// with what `escape_text` escapes.
pub fn quote_text(text: &str) -> String {
  format!("`{}`", escape_text_with(text, true, false))
}

/// Escapes text from the project that's shown as it is (ex. the specifier of an
/// approval's item), so it can't change how what's shown around it reads.
///
/// IMPORTANT: A control character (ex. a newline), a separator other than a
/// plain space (ex. U+00A0 or U+2028), an invisible format character (ex. a
/// bidi override like U+202E, which shows the text after it backwards), any
/// other default ignorable code point (ex. a variation selector), a code
/// point that's unassigned in the version of Unicode this version of dprint
/// has (which a later version of Unicode may add as an invisible format
/// character), and a combining mark at the start (which would change what's
/// shown before it) are shown as an escape of their code point in hex
/// instead (ex. `\u{a}` or `\u{202e}`). They're by their Unicode properties
/// rather than a list, so the editor extensions can escape text the same way
/// (see `is_escaped_in_text`). That's the only kind of escape, so a backslash
/// that isn't followed by `u{` is shown as it is (ex. in a Windows path like
/// `C:\new`), and one that is is shown as `\u{5c}`, so an escape can't be
/// mistaken for the text.
pub fn escape_text(text: &str) -> String {
  escape_text_with(text, false, false)
}

/// Gets whether a character is shown as an escape in text from the project
/// (see `escape_text`), apart from a combining mark at the start, which is
/// one whose general category is `Cc`, `Cf`, `Cn`, `Zl`, `Zp`, or `Zs` other
/// than a plain space, or that's a default ignorable code point.
///
/// IMPORTANT: An unassigned code point (`Cn`) is escaped so that whatever a
/// later version of Unicode adds is escaped by this version (ex. a format
/// character), because the editor extensions don't escape what the server
/// escaped again. The general categories are of the Unicode version of
/// `unicode-general-category` (16), which is part of the approval ids of
/// paths because they have the escaped text (see `get_path_download`).
pub fn is_escaped_in_text(c: char) -> bool {
  match get_general_category(c) {
    GeneralCategory::Control | GeneralCategory::Format | GeneralCategory::Unassigned | GeneralCategory::LineSeparator | GeneralCategory::ParagraphSeparator => {
      true
    }
    GeneralCategory::SpaceSeparator => c != ' ',
    _ => is_default_ignorable_code_point(c),
  }
}

/// Gets whether a character is a combining mark (general category `Mn` or
/// `Me`), which is shown on the character before it.
pub fn is_combining_mark(c: char) -> bool {
  matches!(get_general_category(c), GeneralCategory::NonspacingMark | GeneralCategory::EnclosingMark)
}

/// The domains that are trusted to download from, which includes their
/// subdomains (ex. `githubusercontent.com` includes `raw.githubusercontent.com`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedDomains(Vec<String>);

impl TrustedDomains {
  /// Gets the default trusted domains along with the ones of the
  /// `editor.trustedDomains` setting, which is none at all when the editors
  /// don't use the settings (see `Settings`). It's only a setting and not
  /// something like an environment variable because what the editor is
  /// started with may come from the project (ex. a terminal in the project's
  /// directory).
  pub fn from_settings(environment: &impl Environment) -> Self {
    let settings = match read_settings(environment) {
      Ok(Some(settings)) if settings.get_editor_problem().is_none() => Some(settings),
      Ok(Some(_)) | Err(_) => {
        // IMPORTANT: A property or value in `editor` this version doesn't know may be a
        // setting of a newer version that trusts less (ex. not a default domain), so no
        // domain is trusted then, like when the settings can't be read. A warning for
        // either is logged when reading the permissions.
        return TrustedDomains(Vec::new());
      }
      Ok(None) => None,
    };
    let mut domains = DEFAULT_TRUSTED_DOMAINS.iter().map(|domain| domain.to_string()).collect::<Vec<_>>();
    for domain in settings.iter().flat_map(|settings| &settings.editor.trusted_domains) {
      if let Some(domain) = Self::parse_domain(domain)
        && !domains.contains(&domain)
      {
        domains.push(domain);
      }
    }
    TrustedDomains(domains)
  }

  /// Parses a domain of the `editor.trustedDomains` setting as it's compared
  /// to the host of a url, which is `None` when it isn't a host name (letters,
  /// digits, hyphens, underscores, and dots between them, like the host of a
  /// url may have) or an IP address (an IPv6 one in brackets). The whitespace
  /// and the dots around it are ignored, and so is the case.
  ///
  /// IMPORTANT: Anything else fails instead of being trusted as something that
  /// matches nothing, because it may be a value of a newer version of dprint
  /// that trusts less (ex. `!github.com` to not trust a default domain).
  pub fn parse_domain(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('.').to_ascii_lowercase();
    let is_ip_v6 = value.starts_with('[');
    let is_host_name = !value.is_empty()
      && value.split('.').all(|label| !label.is_empty())
      && value
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'));
    if !is_ip_v6 && !is_host_name {
      return None;
    }
    // it's in the form of the host of a url (ex. an IPv4 address written another way)
    let host = url::Host::parse(&value).ok()?;
    (matches!(host, url::Host::Ipv6(_)) == is_ip_v6).then(|| host.to_string())
  }

  /// Gets whether the url is on a trusted domain, which is only for https
  /// because what's downloaded over http could be changed on the way.
  pub fn is_trusted(&self, url: &Url) -> bool {
    if url.scheme() != "https" {
      return false;
    }
    let Some(host) = url.host_str() else {
      return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    self
      .0
      .iter()
      .any(|domain| host == *domain || host.strip_suffix(domain.as_str()).is_some_and(|prefix| prefix.ends_with('.')))
  }
}

/// What a download that's the proxy of an `.npmrc` file starts with, which
/// isn't mistaken for a url because a url can't have the newline it has.
///
/// IMPORTANT: The forms of the downloads are kept in session files and hashed
/// into approval ids, which other versions read, so what one allows is never
/// broadened, and a broader allow gets a new form (ex. a new prefix) instead,
/// which older versions match against nothing (see `SessionFile`).
const NPMRC_PROXY_DOWNLOAD_PREFIX: &str = "npmrc-proxy:";
/// What starts the download of an npm registry (see `get_npm_registry_download`),
/// whose url ends with a slash, which a reader relies on (see `is_allowed`).
const NPM_REGISTRY_DOWNLOAD_PREFIX: &str = "npm-registry:";

/// How the system resolves the paths that are checked (see `PathRules`).
const PATH_RULES: PathRules = PathRules {
  is_net_automounted: cfg!(any(target_os = "macos", target_os = "linux")),
  is_net_always_automounted: cfg!(target_os = "macos"),
  is_parent_lexical: cfg!(windows),
  has_windows_names: cfg!(windows),
};

/// The most symlinks to follow when resolving a path, which is more than the
/// systems follow, so reading a path with more fails anyway.
const MAX_SYMLINKS: usize = 64;

/// How a system resolves paths, which the tests change to cover each system.
#[derive(Debug, Clone, Copy)]
struct PathRules {
  /// Whether a path in `/net` is on another machine, which is where the
  /// automounter of macOS mounts the shares of a host when the host is
  /// looked up, and the one of Linux does with its `-hosts` map. That's from
  /// its text, which is what the share of the file a path is relative to is
  /// from. A directory of an automounter is also found from its file system,
  /// wherever it is (see `Environment::is_automount_dir`), and so is where a
  /// share is mounted when it's looked up (see
  /// `Environment::is_automount_point`), without mounting anything.
  is_net_automounted: bool,
  /// Whether `/net` is always an automounter's (on macOS), instead of only
  /// when its file system is one (on Linux, which only has one when it's set
  /// up). A share in `/net` is still treated as on another machine either
  /// way, but going up ancestor directories only stops at one when anything
  /// may be read when it is (see `AncestorPaths::parent_on_share`).
  is_net_always_automounted: bool,
  /// Whether `..` is applied to the text of a path before anything in it is
  /// followed, like Win32 does, instead of to where the path before it
  /// resolved to (ex. `link\..\x` is `x` even when `link` is a symlink).
  is_parent_lexical: bool,
  /// Whether a name is resolved like Windows does, where what's before a `:`
  /// is the file of an alternate data stream and Win32 changes some names
  /// (see `WindowsName`).
  has_windows_names: bool,
}

impl PathRules {
  /// Gets the path as the system follows it, which has `.` and `..` applied
  /// to its text when `..` is lexical.
  fn normalize<'a>(&self, path: &'a Path) -> Cow<'a, Path> {
    if self.is_parent_lexical {
      normalize_lexically(path)
    } else {
      Cow::Borrowed(path)
    }
  }
}

/// Gets what's shown and allowed for reading a path on another machine (see
/// `DownloadPolicy::ensure_allows_path`), which is the path along with what it
/// resolved to when that's another path (ex. `/project/link -> /net/host/dir`).
///
/// It's shown as it is (ex. the specifier of an approval's item), so each path
/// is escaped like `quote_text` escapes text. It's also what the approval id
/// has, so what's escaped is part of how the ids are made (see
/// `is_escaped_in_text`).
///
/// IMPORTANT: An arrow in the paths is escaped (`-\u{3e}`) so the one between
/// them is the only one, because otherwise a path from the project could look
/// like it resolved somewhere else (ex. `/net/host/x -> /project/plugin.wasm`).
fn get_path_download(path: &Path, resolved: &Path) -> String {
  let escape_path = |path: &Path| escape_text_with(&path.display().to_string(), true, true);
  let path_text = escape_path(path);
  let resolved_text = escape_path(resolved);
  if resolved_text == path_text {
    path_text
  } else {
    format!("{} -> {}", path_text, resolved_text)
  }
}

/// Quotes a download in a message like `quote_text`, where a path is already
/// escaped (see `get_path_download`).
fn quote_download(download: &str) -> String {
  if is_path_download(download) {
    format!("`{}`", download)
  } else {
    quote_text(get_download_url_text(download))
  }
}

/// Gets whether a download reads an `.npmrc` file on another machine.
fn is_npmrc_path_download(download: &str) -> bool {
  let path = get_path_download_local_path(download).unwrap_or_else(|| PathBuf::from(unescape_text(download)));
  path.file_name().is_some_and(|name| name == ".npmrc")
}

/// Gets where a path that may be on another machine is like
/// `get_network_share`, which follows the path through the file system one
/// component at a time from the root (see `PathWalk`). The followed shares
/// are followed like a local path because their machines are already being
/// read (ex. the share of the config file the path is relative to, or one
/// the user allowed reading).
///
/// IMPORTANT: The text of the path isn't enough because `..`, the case of
/// `/net` on a case insensitive file system, and symlinks (ex. one in the
/// project that points at a UNC path) may reach another machine. Each
/// component is only looked up once the path before it is known to be local
/// or on the followed share because looking up a host in `/net` connects to
/// it, and a symlink is read without following it, so nothing on another
/// machine is looked up.
fn resolve_network_share(
  environment: &impl Environment,
  path: &Path,
  rules: PathRules,
  followed_shares: impl IntoIterator<Item = String>,
) -> Option<NetworkPath> {
  let mut walk = PathWalk::new(followed_shares);
  walk.follow(environment, path, rules, |_| {});
  walk.into_network_path(path)
}

/// Gets the share that an automounter mounted that a directory is on (ex.
/// `/home/me/project` when `/home` is an `autofs` directory), which is
/// followed like the share in the text of a path because it's from the path
/// without any symlinks (see `PathWalk::get_automounted_share`).
fn get_automounted_share(environment: &impl Environment, dir: &Path, rules: PathRules) -> Option<String> {
  let mut walk = PathWalk::new(None);
  walk.follow(environment, dir, rules, |_| {});
  walk.get_automounted_share()
}

/// A path on another machine (see `resolve_network_share`).
struct NetworkPath {
  /// Where it is (see `get_network_share`).
  share: String,
  /// The path through the symlinks it reached the share with, which is what's
  /// read (ex. `\\server\share\plugin.wasm` for a symlink to `\\server\share`).
  resolved: PathBuf,
}

/// Follows a path through the file system one component at a time from the
/// root (see `resolve_network_share`).
#[derive(Debug, Clone, Default)]
struct PathWalk {
  /// The path without symlinks or `..` that's followed so far.
  resolved: PathBuf,
  location: PathLocation,
  /// The root of the share it's on when an automounter mounted it (ex.
  /// `/home/me` when `/home` is an `autofs` directory), which is where what's
  /// on the share is.
  automount_root: Option<PathBuf>,
  symlink_count: usize,
  /// The shares that are followed like a local path, because their machines
  /// are already being read (ex. the share of a project that's on one, or
  /// one the user allowed reading).
  followed_shares: Vec<String>,
  /// Whether the last component that was looked up doesn't exist, so the
  /// path doesn't (see `AncestorPaths::ensure_allows_existing`).
  is_missing: bool,
  /// Whether each directory that was checked is an automounter's, which the
  /// walks that are cloned from this one share (see `Environment::is_automount_dir`).
  automount_dirs: Arc<Mutex<HashMap<PathBuf, bool>>>,
}

/// Where a path that's followed is (see `PathWalk`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum PathLocation {
  #[default]
  Local,
  /// On another machine (see `get_network_share`). What's left of the path is
  /// added to what's followed as it is, apart from a `..` that stays on the
  /// share, because what's on another machine isn't looked up. The followed
  /// shares are the exception, which are followed like a local path.
  Share(String),
  /// It left a share with `..` (ex. `/net/host/..`), which may reach another
  /// machine after already contacting the share's, so it's on another
  /// machine that nothing is the same as. What's left of the path is added as
  /// it is.
  LeftShare,
  /// It couldn't be followed (ex. Win32 changes a name in it), so it may be
  /// on another machine that nothing is the same as. What's left of the path
  /// is added as it is.
  Unresolved,
  /// It went through more symlinks than the systems follow, so reading it
  /// fails anyway.
  SymlinkLoop,
}

impl PathLocation {
  /// Gets where a path that's followed is from its text.
  fn from_path(path: &Path, rules: PathRules) -> Self {
    match get_network_share(path, rules.is_net_automounted) {
      Some(share) => PathLocation::Share(share),
      None => PathLocation::Local,
    }
  }
}

impl PathWalk {
  fn new(followed_shares: impl IntoIterator<Item = String>) -> Self {
    PathWalk {
      followed_shares: followed_shares.into_iter().collect(),
      ..Default::default()
    }
  }

  /// Follows the directory and its ancestors, which gets how each of them was
  /// followed (see `AncestorPaths`).
  fn follow_ancestors(environment: &impl Environment, dir: &Path, rules: PathRules, followed_share: Option<String>) -> HashMap<PathBuf, PathWalk> {
    let dir = rules.normalize(dir);
    // the ancestors in the order their last components are followed
    let mut ancestors = Vec::new();
    let mut ancestor = PathBuf::new();
    for component in dir.components().filter(|component| !matches!(component, Component::CurDir)) {
      ancestor.push(component);
      ancestors.push(ancestor.clone());
    }
    let mut ancestors = ancestors.into_iter();
    let mut walks = HashMap::new();
    let mut walk = PathWalk::new(followed_share);
    walk.follow(environment, &dir, rules, |walk| {
      if let Some(ancestor) = ancestors.next() {
        walks.insert(ancestor, walk.clone());
      }
    });
    walks
  }

  /// Follows the components of the path from where the walk is, which calls
  /// `on_component` once each of them was followed (along with what a symlink
  /// it was points at).
  fn follow(&mut self, environment: &impl Environment, path: &Path, rules: PathRules, mut on_component: impl FnMut(&PathWalk)) {
    let path = rules.normalize(path);
    // the components that are left with the next one last, where the ones of
    // the path are below the ones of the symlinks it went through
    let mut remaining = path.components().rev().filter_map(OwnedComponent::new).collect::<Vec<_>>();
    let mut path_components_left = remaining.len();
    let mut is_component_pending = false;
    loop {
      if self.location == PathLocation::SymlinkLoop {
        return;
      }
      if is_component_pending && remaining.len() == path_components_left {
        is_component_pending = false;
        on_component(self);
      }
      let Some(component) = remaining.pop() else {
        return;
      };
      if remaining.len() < path_components_left {
        path_components_left = remaining.len();
        is_component_pending = true;
      }
      self.step(environment, component, &mut remaining, rules);
    }
  }

  fn step(&mut self, environment: &impl Environment, component: OwnedComponent, remaining: &mut Vec<OwnedComponent>, rules: PathRules) {
    self.is_missing = false;
    match &self.location {
      PathLocation::Local => {}
      PathLocation::Share(share) if self.followed_shares.contains(share) => {}
      PathLocation::Share(_) => {
        if let OwnedComponent::Parent = component {
          let parent = self.resolved.parent().map(|parent| (parent.to_path_buf(), self.get_location(parent, rules).0));
          match parent {
            // IMPORTANT: The system doesn't apply the `..` of a verbatim path (ex.
            // `\\?\UNC\server\share\..`), so where one that would leave its root is read from isn't known.
            None if is_verbatim(&self.resolved) => {
              push_parent_dir(&mut self.resolved);
              self.set_location(PathLocation::LeftShare, None);
            }
            // ex. the root of a UNC path, which can't be left
            None => {}
            Some((parent, location)) if location == self.location => self.resolved = parent,
            // IMPORTANT: Leaving the share (ex. `/net/host/..`) may reach another machine.
            Some(_) => {
              push_parent_dir(&mut self.resolved);
              self.set_location(PathLocation::LeftShare, None);
            }
          }
        } else {
          component.push_to(&mut self.resolved);
        }
        return;
      }
      PathLocation::LeftShare | PathLocation::Unresolved => {
        component.push_to(&mut self.resolved);
        return;
      }
      PathLocation::SymlinkLoop => return,
    }
    match component {
      OwnedComponent::Root(root) => {
        self.resolved.push(root);
        // ex. a UNC prefix, or an absolute symlink on the followed share that points at a local path
        self.set_location(PathLocation::from_path(&self.resolved, rules), None);
      }
      OwnedComponent::Parent => {
        // the parent of the root is the root
        if !self.resolved.pop() && is_verbatim(&self.resolved) {
          // IMPORTANT: The system doesn't apply the `..` of a verbatim path (ex.
          // `\\?\Volume{...}\..\UNC\server\share`), so where one that would leave its root is read from isn't known.
          push_parent_dir(&mut self.resolved);
          self.set_location(PathLocation::Unresolved, None);
          return;
        }
        // ex. it left the followed share (`/net/host/..`), which is followed like a local path
        let (location, automount_root) = self.get_location(&self.resolved, rules);
        self.set_location(location, automount_root);
      }
      // it's of the file or directory that was followed, so it's where that is
      OwnedComponent::Stream(stream) => append_stream(&mut self.resolved, &stream),
      OwnedComponent::Normal(name) => self.step_into(environment, &name, remaining, rules),
    }
  }

  /// Follows a name in the directory that was followed.
  fn step_into(&mut self, environment: &impl Environment, name: &OsStr, remaining: &mut Vec<OwnedComponent>, rules: PathRules) {
    let next = self.resolved.join(name);
    let (mut location, mut automount_root) = self.get_location(&next, rules);
    // IMPORTANT: Looking up what's in an automounter's directory (ex. `/net` with `autofs`) may
    // mount it from another machine. The share is the path as it is because it may be case sensitive.
    if location == PathLocation::Local && self.is_automount_dir(environment, &self.resolved) {
      location = PathLocation::Share(next.to_string_lossy().into_owned());
      automount_root = Some(next.clone());
    }
    if matches!(&location, PathLocation::Share(share) if !self.followed_shares.contains(share)) {
      self.resolved = next;
      self.set_location(location, automount_root);
      return;
    }
    let windows_name = if rules.has_windows_names {
      WindowsName::new(&self.resolved, name)
    } else {
      WindowsName::Plain
    };
    // what's looked up, which is the file of a stream because looking up the stream of a symlink
    // fails although reading it follows the symlink
    let (lookup_path, stream) = match windows_name {
      WindowsName::Plain => (next.clone(), None),
      // the stream of the directory that was followed (ex. `dir\:stream`)
      WindowsName::Stream("", _) => {
        self.resolved = next;
        self.set_location(location, automount_root);
        return;
      }
      WindowsName::Stream(file_name, stream) => (self.resolved.join(file_name), Some(stream)),
      // IMPORTANT: Win32 doesn't change it the same way when it's looked up as when it's read.
      WindowsName::Changed => {
        self.resolved = next;
        self.set_location(PathLocation::Unresolved, None);
        return;
      }
    };
    let kind = environment.path_kind(&lookup_path);
    if kind != Some(PathKind::Symlink) {
      // IMPORTANT: The file of a stream that can't be looked up may be a symlink that reading the stream follows.
      if stream.is_some() && kind.is_none() && environment.path_kind(&next).is_some() {
        self.resolved = next;
        self.set_location(PathLocation::Unresolved, None);
        return;
      }
      // IMPORTANT: Looking up what's in where a share is mounted when it's looked up (ex. an AFS
      // cell on Linux) mounts it from another machine. The share is the path as it is like above.
      if kind == Some(PathKind::Dir) && location == PathLocation::Local && environment.is_automount_point(&lookup_path) {
        location = PathLocation::Share(next.to_string_lossy().into_owned());
        automount_root = Some(next.clone());
      }
      self.is_missing = kind.is_none();
      self.resolved = next;
      self.set_location(location, automount_root);
      return;
    }
    self.symlink_count += 1;
    if self.symlink_count > MAX_SYMLINKS {
      self.set_location(PathLocation::SymlinkLoop, None);
      return;
    }
    match environment.read_link(&lookup_path) {
      Ok(target) => {
        let target = get_symlink_target_path(&self.resolved, &target, rules);
        // the stream is of what the symlink points at, so it's added once that's followed
        if let Some(stream) = stream {
          remaining.push(OwnedComponent::Stream(stream.into()));
        }
        remaining.extend(target.components().rev().filter_map(OwnedComponent::new));
        self.resolved = PathBuf::new();
      }
      // reading the path fails the same way
      Err(_) => {
        self.resolved = next;
        self.set_location(location, automount_root);
      }
    }
  }

  /// Gets where a path the walk goes to from where it is would be from its
  /// text, which is on the share an automounter mounted that the walk is on
  /// when it's in the share's root, along with that root.
  fn get_location(&self, path: &Path, rules: PathRules) -> (PathLocation, Option<PathBuf>) {
    match &self.automount_root {
      Some(root) if path.starts_with(root) => (self.location.clone(), Some(root.clone())),
      _ => (PathLocation::from_path(path, rules), None),
    }
  }

  /// Sets where the walk is, with the root of the share an automounter
  /// mounted when it's on one.
  fn set_location(&mut self, location: PathLocation, automount_root: Option<PathBuf>) {
    self.location = location;
    self.automount_root = automount_root;
  }

  /// Gets whether the directory is an automounter's (see
  /// `Environment::is_automount_dir`), which is only checked once.
  fn is_automount_dir(&self, environment: &impl Environment, dir: &Path) -> bool {
    if dir.as_os_str().is_empty() {
      return false;
    }
    if let Some(is_automount_dir) = self.automount_dirs.lock().get(dir) {
      return *is_automount_dir;
    }
    let is_automount_dir = environment.is_automount_dir(dir);
    self.automount_dirs.lock().insert(dir.to_path_buf(), is_automount_dir);
    is_automount_dir
  }

  /// Gets the share an automounter mounted that the path that was followed is
  /// on, which is only when no symlink was followed to get to it, so it's on
  /// it like a path is on the share in its text.
  fn get_automounted_share(&self) -> Option<String> {
    match (&self.location, &self.automount_root) {
      (PathLocation::Share(share), Some(_)) if self.symlink_count == 0 => Some(share.clone()),
      _ => None,
    }
  }

  /// Gets where the path that was followed is when it's on another machine.
  fn into_network_path(self, path: &Path) -> Option<NetworkPath> {
    match self.location {
      PathLocation::Local => None,
      PathLocation::Share(share) => Some(NetworkPath {
        share,
        resolved: self.resolved,
      }),
      // the whole path, which the file it's relative to never matches
      PathLocation::LeftShare | PathLocation::Unresolved => Some(NetworkPath {
        share: self.resolved.to_string_lossy().to_lowercase(),
        resolved: self.resolved,
      }),
      PathLocation::SymlinkLoop => Some(NetworkPath {
        share: path.to_string_lossy().to_lowercase(),
        resolved: path.to_path_buf(),
      }),
    }
  }
}

/// A component of a path that's owned (see `resolve_network_share`).
enum OwnedComponent {
  /// A prefix (ex. `C:` or a UNC prefix on Windows) or the root directory.
  Root(OsString),
  Parent,
  Normal(OsString),
  /// An alternate data stream (ex. `:stream`) of the file or directory
  /// before it on Windows, which is added to its name (see `WindowsName`).
  Stream(OsString),
}

impl OwnedComponent {
  /// Gets the owned component, which is `None` for `.` because it's the same directory.
  fn new(component: Component<'_>) -> Option<Self> {
    match component {
      Component::Prefix(_) | Component::RootDir => Some(OwnedComponent::Root(component.as_os_str().to_os_string())),
      Component::CurDir => None,
      Component::ParentDir => Some(OwnedComponent::Parent),
      Component::Normal(name) => Some(OwnedComponent::Normal(name.to_os_string())),
    }
  }

  /// Adds the component to the path as it is (ex. `..` isn't applied).
  fn push_to(self, path: &mut PathBuf) {
    match self {
      OwnedComponent::Root(root) => path.push(root),
      OwnedComponent::Parent => push_parent_dir(path),
      OwnedComponent::Normal(name) => path.push(name),
      OwnedComponent::Stream(stream) => append_stream(path, &stream),
    }
  }
}

/// How Windows resolves a name of a path (see `PathRules::has_windows_names`).
enum WindowsName<'a> {
  /// It's the file or directory with the name.
  Plain,
  /// It's an alternate data stream of the file or directory with the name
  /// before the `:` (ex. `link:stream` or `dir::$INDEX_ALLOCATION`), which is
  /// the file name and the stream with its `:`.
  Stream(&'a str, &'a str),
  /// Win32 changes it, which it doesn't always do the same way when it's
  /// looked up as when it's read (ex. it removes a trailing space from the
  /// last component only), or opens a device for it (ex. `LPT1`, which may be
  /// a printer on another machine).
  Changed,
}

impl<'a> WindowsName<'a> {
  /// Gets how Windows resolves the name in the directory, where Win32 doesn't
  /// change a name in a verbatim path (ex. `\\?\C:\dir`).
  fn new(dir: &Path, name: &'a OsStr) -> Self {
    let Some(name) = name.to_str() else {
      // a name that isn't unicode isn't split, so it's only known when it doesn't have a stream
      return if name.as_encoded_bytes().contains(&b':') {
        WindowsName::Changed
      } else {
        WindowsName::Plain
      };
    };
    let (file_name, stream) = match name.find(':') {
      Some(index) => (&name[..index], Some(&name[index..])),
      None => (name, None),
    };
    if !is_verbatim(dir) && (file_name.ends_with(['.', ' ']) || is_dos_device_name(file_name)) {
      return WindowsName::Changed;
    }
    match stream {
      Some(stream) => WindowsName::Stream(file_name, stream),
      None => WindowsName::Plain,
    }
  }
}

/// Gets whether Win32 opens a device for a name (ex. `NUL` or `COM1`), which
/// older versions of Windows also do when it has an extension (ex. `nul.txt`).
fn is_dos_device_name(file_name: &str) -> bool {
  let stem = file_name.split('.').next().unwrap_or(file_name).trim_end_matches(' ').to_ascii_uppercase();
  match stem.as_str() {
    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
    _ => {
      (stem.starts_with("COM") || stem.starts_with("LPT"))
        && matches!(
          &stem[3..],
          "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "\u{b9}" | "\u{b2}" | "\u{b3}"
        )
    }
  }
}

/// Adds an alternate data stream (ex. `:stream`) to the last component of a
/// path, which is the file or directory it's of.
fn append_stream(path: &mut PathBuf, stream: &OsStr) {
  match path.file_name() {
    Some(name) => {
      let mut name = name.to_os_string();
      name.push(stream);
      path.set_file_name(name);
    }
    // ex. the root of a drive
    None => path.push(stream),
  }
}

/// Adds `..` to the text of a path without applying it, which `PathBuf::push`
/// does to a verbatim path (ex. `\\?\C:\` stays the same).
fn push_parent_dir(path: &mut PathBuf) {
  let mut text = std::mem::take(path).into_os_string();
  let needs_separator = text.as_encoded_bytes().last().is_some_and(|last| !std::path::is_separator(*last as char));
  if needs_separator {
    text.push(std::path::MAIN_SEPARATOR_STR);
  }
  text.push("..");
  *path = PathBuf::from(text);
}

/// Gets the path a symlink in the directory points at, where a relative target
/// is relative to the directory and an absolute one replaces it.
///
/// IMPORTANT: Win32 applies the `..` of a target to its text too (ex. a target
/// of `link\..\x` is the directory's `x` even when `link` is a symlink).
fn get_symlink_target_path(dir: &Path, target: &Path, rules: PathRules) -> PathBuf {
  let target = dir.join(target);
  match rules.normalize(&target) {
    Cow::Borrowed(_) => target,
    Cow::Owned(normalized) => normalized,
  }
}

/// Applies the `.` and `..` of a path to its text like Win32 does before it
/// opens a path, where the parent of the root is the root. A verbatim path
/// (ex. `\\?\C:\dir`) is used as it is.
///
/// IMPORTANT: The root of a device path is `\\.\`, so a `..` may remove its
/// device (ex. `\\.\Volume{...}\..\UNC\server\share` is `\\.\UNC\server\share`).
fn normalize_lexically(path: &Path) -> Cow<'_, Path> {
  let mut components = path.components();
  match components.next() {
    Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim() => return Cow::Borrowed(path),
    Some(Component::Prefix(prefix)) if let Prefix::DeviceNS(device) = prefix.kind() => {
      return Cow::Owned(normalize_device_path_lexically(device, components));
    }
    _ => {}
  }
  if !path.components().any(|component| matches!(component, Component::CurDir | Component::ParentDir)) {
    return Cow::Borrowed(path);
  }
  let mut normalized = PathBuf::new();
  for component in path.components() {
    match component {
      Component::CurDir => {}
      Component::ParentDir => match normalized.components().next_back() {
        Some(Component::Normal(_)) => {
          normalized.pop();
        }
        Some(Component::RootDir) => {}
        Some(Component::Prefix(_)) if normalized.has_root() => {}
        // ex. a relative path that starts above its directory
        _ => normalized.push(".."),
      },
      component => normalized.push(component),
    }
  }
  Cow::Owned(normalized)
}

/// Applies the `.` and `..` of a device path (ex. `\\.\Volume{...}\dir`) like
/// `GetFullPathNameW` does, where the device is a name in `\\.\` like the
/// ones after it.
fn normalize_device_path_lexically(device: &OsStr, components: std::path::Components<'_>) -> PathBuf {
  let mut has_root = false;
  let mut names: Vec<&OsStr> = Vec::new();
  let rest = components.filter_map(|component| match component {
    Component::RootDir => {
      has_root = true;
      None
    }
    component => Some(component.as_os_str()),
  });
  for name in std::iter::once(device).chain(rest) {
    match name.as_encoded_bytes() {
      b"." => {}
      // the parent of the root is the root
      b".." => {
        names.pop();
      }
      _ => names.push(name),
    }
  }
  let mut text = OsString::from(r"\\.\");
  for (index, name) in names.iter().enumerate() {
    if index > 0 {
      text.push(r"\");
    }
    text.push(name);
  }
  // ex. the root directory of a volume, which isn't the volume
  if has_root && names.len() == 1 {
    text.push(r"\");
  }
  PathBuf::from(text)
}

/// Gets the parent of the directory from its text like
/// `AncestorPaths::parent_on_share`.
///
/// The root of a UNC path has no parent, and a share that isn't known (ex.
/// `\\?\GLOBALROOT\Device\Mup\server\share`) is the whole path, so it
/// only stops at the root of a share in `/net`.
fn get_parent_on_share_with(dir: &Path, is_net_automounted: bool) -> Option<&Path> {
  let parent = dir.parent()?;
  let is_net_share_root = dir.components().next() == Some(Component::RootDir)
    && get_network_share(dir, is_net_automounted).is_some()
    && get_network_share(parent, is_net_automounted).is_none();
  (!is_net_share_root).then_some(parent)
}

/// Gets where a path that may be on another machine is (ex. the server and
/// share of a UNC path), lowercased because it's case insensitive. It's only
/// from the text of the path, so the path needs to be resolved first (see
/// `resolve_network_share`).
///
/// IMPORTANT: On Windows, only a path on a drive or a volume (ex.
/// `\\?\Volume{...}\`, which is a drive without a letter) is known to be
/// local. A UNC path isn't, and neither is any other kind of prefix because
/// some of them also reach another machine (ex. `\\.\UNC\server\share` or
/// `\\?\GLOBALROOT\Device\Mup\server\share`). That includes a volume in a
/// device path (`\\.\Volume{...}\`), whose `..` Win32 applies to the volume
/// too (see `normalize_lexically`). On macOS and Linux, a path in
/// `/net` is on another machine because the automounter mounts it (when
/// Linux has one), and so is one in `/System/Volumes/Data/net`, which is
/// where `/net` is on the data volume of macOS (it's treated the same even
/// where it isn't, which is safe).
fn get_network_share(path: &Path, is_net_automounted: bool) -> Option<String> {
  match path.components().next()? {
    Component::Prefix(prefix) => match prefix.kind() {
      Prefix::Disk(_) | Prefix::VerbatimDisk(_) => None,
      Prefix::Verbatim(name) if is_volume_name(name) => None,
      Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Some(format!(
        "{}\\{}",
        server.to_string_lossy().to_lowercase(),
        share.to_string_lossy().to_lowercase()
      )),
      // the share isn't known, so this is the whole path, which the file it's relative to never matches
      _ => Some(path.to_string_lossy().to_lowercase()),
    },
    Component::RootDir if is_net_automounted => {
      let mut components = path.components().skip(1).peekable();
      // the file system is case insensitive by default, which folds the case of any letter (ex. `ſ` is `s`)
      let is_name = |component: Option<Component<'_>>, name: &str| matches!(component, Some(Component::Normal(value)) if eq_ignore_case(value, name));
      if components.peek().is_some_and(|component| is_name(Some(*component), "System")) {
        for name in ["System", "Volumes", "Data"] {
          if !is_name(components.next(), name) {
            return None;
          }
        }
      }
      match (components.next(), components.next()) {
        (net, Some(Component::Normal(host))) if is_name(net, "net") => Some(format!("/net/{}", host.to_string_lossy().to_lowercase())),
        _ => None,
      }
    }
    _ => None,
  }
}

/// Gets whether a path is verbatim (ex. `\\?\C:\dir`), which Win32 opens as
/// it is.
fn is_verbatim(path: &Path) -> bool {
  matches!(path.components().next(), Some(Component::Prefix(prefix)) if prefix.kind().is_verbatim())
}

/// Gets whether a prefix's name is the name of a volume (ex. `Volume{...}`).
fn is_volume_name(name: &OsStr) -> bool {
  name
    .to_str()
    .and_then(|name| name.get(..7))
    .is_some_and(|start| start.eq_ignore_ascii_case("Volume{"))
    && name.as_encoded_bytes().ends_with(b"}")
}

/// Gets whether a name is the same as another without their case, where a
/// letter is the same as each one that its case folds to (ex. `ſ` is `s`
/// and `S`), like a case insensitive file system compares them. Folding more
/// than one does only treats more paths as on another machine.
fn eq_ignore_case(value: &OsStr, name: &str) -> bool {
  let fold = |text: &str| text.chars().flat_map(char::to_uppercase).flat_map(char::to_lowercase).collect::<String>();
  value.to_str().is_some_and(|value| fold(value) == fold(name))
}

/// Escapes text from the project like `escape_text`, which also escapes a
/// backtick and what looks like one for `quote_text`, and the `>` of an arrow
/// (`->`) for `get_path_download`.
fn escape_text_with(text: &str, escape_backticks: bool, escape_arrows: bool) -> String {
  let mut escaped = String::with_capacity(text.len());
  let mut previous = None;
  for (index, c) in text.char_indices() {
    let is_escaped = is_escaped_in_text(c)
      || (index == 0 && is_combining_mark(c))
      || (c == '\\' && text[index + 1..].starts_with("u{"))
      || (escape_backticks && is_backtick_like(c))
      || (escape_arrows && c == '>' && previous == Some('-'));
    if is_escaped {
      escaped.extend(c.escape_unicode());
    } else {
      escaped.push(c);
    }
    previous = Some(c);
  }
  escaped
}

/// Gets the text that was escaped like `escape_text` (ex. a path of
/// `get_path_download`), where each escape is a code point in hex.
fn unescape_text(text: &str) -> String {
  let mut unescaped = String::with_capacity(text.len());
  let mut rest = text;
  while let Some(index) = rest.find("\\u{") {
    unescaped.push_str(&rest[..index]);
    let after = &rest[index + 3..];
    let c = after
      .split_once('}')
      .and_then(|(hex, _)| u32::from_str_radix(hex, 16).ok())
      .and_then(char::from_u32);
    match c {
      Some(c) => {
        unescaped.push(c);
        rest = &after[after.find('}').unwrap() + 1..];
      }
      // not an escape, which escaped text doesn't have
      None => {
        unescaped.push_str("\\u{");
        rest = after;
      }
    }
  }
  unescaped.push_str(rest);
  unescaped
}

/// Gets whether a character is a default ignorable code point that isn't a
/// format character (`Cf`), which `is_escaped_in_text` escapes as well.
/// These are the `Other_Default_Ignorable_Code_Point` and
/// `Variation_Selector` code points of Unicode's `PropList.txt`, which
/// `Default_Ignorable_Code_Point` is derived from along with the format
/// characters, including the ones that are reserved for them (ex. the rest of
/// U+E0000 to U+E0FFF).
fn is_default_ignorable_code_point(c: char) -> bool {
  matches!(
    c,
    '\u{34f}'
      | '\u{115f}'..='\u{1160}'
      | '\u{17b4}'..='\u{17b5}'
      | '\u{180b}'..='\u{180d}'
      | '\u{180f}'
      | '\u{2065}'
      | '\u{3164}'
      | '\u{fe00}'..='\u{fe0f}'
      | '\u{ffa0}'
      | '\u{fff0}'..='\u{fff8}'
      | '\u{e0000}'..='\u{e0fff}'
  )
}

/// Gets whether a character is a backtick or looks like one, which is shown
/// as an escape in quoted text (see `quote_text`).
fn is_backtick_like(c: char) -> bool {
  matches!(
    c,
    '`' | '\u{2cb}' | '\u{2ce}' | '\u{300}' | '\u{316}' | '\u{340}' | '\u{1fef}' | '\u{2018}' | '\u{201b}' | '\u{2035}' | '\u{ff40}'
  )
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;

  /// The paths of macOS, which has `/net` (and how Linux applies `..`).
  const MACOS: PathRules = PathRules {
    is_net_automounted: true,
    is_net_always_automounted: true,
    is_parent_lexical: false,
    has_windows_names: false,
  };
  /// The paths of Windows, which applies `..` to the text of a path.
  const WINDOWS: PathRules = PathRules {
    is_net_automounted: false,
    is_net_always_automounted: false,
    is_parent_lexical: true,
    has_windows_names: true,
  };
  /// Applies `..` like Windows with `/net` like macOS, so how Windows applies
  /// `..` is tested on every system.
  const LEXICAL_NET: PathRules = PathRules {
    is_net_automounted: true,
    is_net_always_automounted: true,
    is_parent_lexical: true,
    has_windows_names: false,
  };

  fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
  }

  #[test]
  fn should_allow_a_symlink_to_the_share_of_the_path_it_is_relative_to() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/").unwrap();
    let share_dir = if cfg!(windows) { r"\\server\share\work" } else { "/net/server/share/work" };
    // ex. a workspace folder that's a symlink to a share, which the editor knows the files by
    environment.symlink(share_dir, "/work").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    assert!(policy.ensure_allows_path(&environment, Path::new("/work"), Some(Path::new("/"))).is_err());
    // following it doesn't contact another machine than the one of the directory it's an alias of
    assert!(policy.ensure_allows_path(&environment, Path::new("/work"), Some(Path::new(share_dir))).is_ok());
    let other_share_dir = if cfg!(windows) { r"\\other\share\work" } else { "/net/other/share/work" };
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new("/work"), Some(Path::new(other_share_dir)))
        .is_err()
    );
  }

  #[test]
  fn should_keep_denying_what_is_in_a_denied_directory_under_one_that_is_allowed() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/project").unwrap();
    let share_dir = if cfg!(windows) { r"\\server\share\outer" } else { "/net/server/share/outer" };
    environment.symlink(share_dir, "/project/outer").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let ensure_allows = |path: &str| policy.ensure_allows_path(&environment, Path::new(path), None);
    let inner_download = ensure_allows("/project/outer/inner").unwrap_err().download;
    let outer_download = ensure_allows("/project/outer").unwrap_err().download;
    // the user denied the inner directory for one config file and allowed the outer one for another
    policy.set_denied_downloads([inner_download]);
    policy.allow(&[outer_download]);
    assert!(ensure_allows("/project/outer/inner/plugin.wasm").is_err());
    assert!(ensure_allows("/project/outer/inner").is_err());
    assert!(ensure_allows("/project/outer/other/plugin.wasm").is_ok());
    // but allowing a file in it exactly wins
    let file_download = ensure_allows("/project/outer/inner/plugin.wasm").unwrap_err().download;
    policy.allow(&[file_download]);
    assert!(ensure_allows("/project/outer/inner/plugin.wasm").is_ok());
  }

  #[test]
  fn should_compare_a_path_download_by_its_paths() {
    // a text another version escaped differently (ex. with another version of Unicode)
    let download = get_path_download(Path::new("/project/pkg"), Path::new("/net/server/share/pkg"));
    let escaped = download.replace("pkg", "p\\u{6b}g");
    assert_ne!(escaped, download);
    assert_eq!(normalize_download(&escaped), download);
    assert_eq!(normalize_download("/net/server/share/p\\u{6b}g"), "/net/server/share/pkg");
    // an arrow in a path is still escaped
    let download = get_path_download(Path::new("/project/a->b"), Path::new("/net/server/share/a->b"));
    assert_eq!(normalize_download(&download), download);
    // what isn't a path is kept
    assert_eq!(normalize_download("https://example.com/p\\u{6b}g"), "https://example.com/p\\u{6b}g");
  }

  #[test]
  fn should_keep_denying_a_file_in_a_directory_that_is_allowed() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/project").unwrap();
    let share_dir = if cfg!(windows) { r"\\server\share\pkg" } else { "/net/server/share/pkg" };
    environment.symlink(share_dir, "/project/pkg").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let ensure_allows = |path: &str| policy.ensure_allows_path(&environment, Path::new(path), None);
    let file_download = ensure_allows("/project/pkg/plugin.wasm").unwrap_err().download;
    let dir_download = ensure_allows("/project/pkg").unwrap_err().download;
    // the user denied the file for one config file and allowed its directory for another
    policy.set_denied_downloads([file_download.clone()]);
    policy.allow(&[dir_download]);
    assert!(ensure_allows("/project/pkg/plugin.wasm").is_err());
    assert!(ensure_allows("/project/pkg/other.wasm").is_ok());
    // but allowing it exactly wins
    policy.allow(&[file_download]);
    assert!(ensure_allows("/project/pkg/plugin.wasm").is_ok());
  }

  #[test]
  fn should_only_allow_exactly_what_the_user_allowed() {
    let environment = TestEnvironment::new();
    let policy = DownloadPolicy::for_editor(&environment);
    assert!(policy.ensure_allows(&url("https://plugins.dprint.dev/plugin.wasm")).is_ok());
    let err = policy.ensure_allows(&url("https://example.com/plugin.wasm")).unwrap_err();
    assert_eq!(err.download, "https://example.com/plugin.wasm");
    policy.allow(&[
      "https://example.com/plugin.wasm".to_string(),
      "https://origin.example.com/".to_string(),
      get_npm_registry_download(&url("https://npm.example.com")),
      get_npm_registry_download(&url("https://corp.example.com/npm/repo")),
    ]);
    assert!(policy.allows(&url("https://example.com/plugin.wasm")));
    // not what redirects to
    assert!(!policy.allows(&url("https://example.com/other.wasm")));
    // a url ending with a slash only allows itself (ex. what a config file extends)
    assert!(policy.allows(&url("https://origin.example.com/")));
    assert!(!policy.allows(&url("https://origin.example.com/other.json")));
    // an npm registry allows what's under it
    assert!(policy.ensure_allows_npm_registry(&url("https://npm.example.com/")).is_ok());
    assert!(policy.allows(&url("https://npm.example.com/@scope/package")));
    assert!(!policy.allows(&url("https://npm.example.com.evil.com/@scope/package")));
    assert!(policy.allows(&url("https://corp.example.com/npm/repo/package")));
    assert!(!policy.allows(&url("https://corp.example.com/npm/repository")));
    let err = policy.ensure_allows_npm_registry(&url("https://other.example.com/")).unwrap_err();
    assert_eq!(err.download, "npm-registry:https://other.example.com/");
    assert_eq!(
      err.to_string(),
      "Downloading `https://other.example.com/` needs to be allowed because its domain isn't trusted."
    );
    assert!(policy.ensure_allows_npm_registry(&url("https://registry.npmjs.org/")).is_ok());
    // a denial wins over a registry, but not over an exact allow
    policy.set_denied_downloads(["https://npm.example.com/a.json".to_string(), "https://example.com/plugin.wasm".to_string()]);
    assert!(!policy.allows(&url("https://npm.example.com/a.json")));
    assert!(policy.allows(&url("https://npm.example.com/b.json")));
    assert!(policy.allows(&url("https://example.com/plugin.wasm")));
    policy.set_denied_downloads([]);
    // a registry url without a slash (ex. from a corrupted session file) isn't a prefix
    policy.allow(&["npm-registry:https://npm.corp".to_string()]);
    assert!(!policy.allows(&url("https://npm.corp.example/package")));
    assert!(!policy.allows(&url("https://npm.corp/package")));
    // the cli downloads anything
    assert!(DownloadPolicy::All.allows(&url("http://example.com/plugin.wasm")));
  }

  #[cfg(windows)]
  #[test]
  fn should_not_allow_reading_from_another_machine() {
    let environment = TestEnvironment::new();
    let policy = DownloadPolicy::for_editor(&environment);
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new(r"C:\project\plugin.wasm"), Some(Path::new(r"C:\project")))
        .is_ok()
    );
    let err = policy
      .ensure_allows_path(&environment, Path::new(r"\\server\share\plugin.wasm"), Some(Path::new(r"C:\project")))
      .unwrap_err();
    assert_eq!(err.download, r"\\server\share\plugin.wasm");
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new(r"\\?\UNC\server\share\plugin.wasm"), None)
        .is_err()
    );
    // other ways of reaching another machine
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new(r"\\.\UNC\server\share\plugin.wasm"), None)
        .is_err()
    );
    assert!(
      policy
        .ensure_allows_path(
          &environment,
          Path::new(r"\\?\GLOBALROOT\Device\Mup\server\share\plugin.wasm"),
          Some(Path::new(r"C:\project"))
        )
        .is_err()
    );
    assert!(policy.ensure_allows_path(&environment, Path::new(r"\\?\C:\project\plugin.wasm"), None).is_ok());
    // a config file on the share may use what's on it
    assert!(
      policy
        .ensure_allows_path(
          &environment,
          Path::new(r"\\SERVER\share\plugin.wasm"),
          Some(Path::new(r"\\server\Share\project"))
        )
        .is_ok()
    );
    assert!(
      policy
        .ensure_allows_path(
          &environment,
          Path::new(r"\\server\other\plugin.wasm"),
          Some(Path::new(r"\\server\share\project"))
        )
        .is_err()
    );
    policy.allow(&[r"\\server\share\plugin.wasm".to_string()]);
    assert!(policy.ensure_allows_path(&environment, Path::new(r"\\server\share\plugin.wasm"), None).is_ok());
    // the cli reads anything
    assert!(
      DownloadPolicy::All
        .ensure_allows_path(&environment, Path::new(r"\\server\share\plugin.wasm"), None)
        .is_ok()
    );
  }

  #[cfg(windows)]
  #[test]
  fn should_not_allow_reading_from_another_machine_through_a_symlink() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/project").unwrap();
    environment.symlink(r"\\server\share\dir", "/project/link").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let err = policy
      .ensure_allows_path(&environment, Path::new("/project/link/plugin.wasm"), Some(Path::new("/project")))
      .unwrap_err();
    // it's shown and allowed with the machine it's on
    let resolved = Path::new(r"\\server\share\dir").join("plugin.wasm");
    assert_eq!(err.download, format!("/project/link/plugin.wasm -> {}", resolved.display()));
    policy.allow(std::slice::from_ref(&err.download));
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new("/project/link/plugin.wasm"), Some(Path::new("/project")))
        .is_ok()
    );
    // which is asked about again when a symlink points elsewhere
    environment.mk_dir_all("/project/dir").unwrap();
    environment.symlink(r"\\server\share\dir", "/project/dir/hop").unwrap();
    environment.symlink("/project/dir/hop", "/project/other_link").unwrap();
    let err = policy
      .ensure_allows_path(&environment, Path::new("/project/other_link/plugin.wasm"), Some(Path::new("/project")))
      .unwrap_err();
    assert_eq!(err.download, format!("/project/other_link/plugin.wasm -> {}", resolved.display()));
    policy.allow(std::slice::from_ref(&err.download));
    environment.remove_dir_all("/project/dir").unwrap();
    environment.mk_dir_all("/project/dir").unwrap();
    environment.symlink(r"\\other\share", "/project/dir/hop").unwrap();
    let err = policy
      .ensure_allows_path(&environment, Path::new("/project/other_link/plugin.wasm"), Some(Path::new("/project")))
      .unwrap_err();
    let resolved = Path::new(r"\\other\share").join("plugin.wasm");
    assert_eq!(err.download, format!("/project/other_link/plugin.wasm -> {}", resolved.display()));
    // a config file on the share may use what's on it
    let policy = DownloadPolicy::for_editor(&environment);
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new("/project/link/plugin.wasm"), Some(Path::new(r"\\Server\share\project")))
        .is_ok()
    );
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new("/project/link/plugin.wasm"), Some(Path::new(r"\\server\other\project")))
        .is_err()
    );
    // allowing a directory allows what's in it where it resolved to (ex. the files of an npm package)
    let err = policy.ensure_allows_path(&environment, Path::new("/project/link"), None).unwrap_err();
    assert_eq!(err.download, r"/project/link -> \\server\share\dir");
    policy.allow(&[err.download]);
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new("/project/link/sub/plugin.wasm"), None)
        .is_ok()
    );
    // but not what's outside of it
    environment.symlink(r"\\server\share", "/project/share_link").unwrap();
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new("/project/share_link/plugin.wasm"), None)
        .is_err()
    );
    // where `..` is applied to the text first, so this is the local `\project\plugin.wasm`
    assert!(policy.ensure_allows_path(&environment, Path::new("/project/link/../plugin.wasm"), None).is_ok());
  }

  #[test]
  fn should_resolve_the_path_to_net_before_reading_it() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/home/me/project").unwrap();
    environment.mk_dir_all("/a/b").unwrap();
    let share = |path: &str| resolve_network_share(&environment, Path::new(path), MACOS, None).map(|network_path| network_path.share);
    let resolved = |path: &str| resolve_network_share(&environment, Path::new(path), MACOS, None).map(|network_path| network_path.resolved);
    assert_eq!(share("/home/me/project/plugin.wasm"), None);
    assert_eq!(share("/net/host/share/plugin.wasm").as_deref(), Some("/net/host"));
    // the parent directories
    assert_eq!(share("/home/me/project/../../../net/host/plugin.wasm").as_deref(), Some("/net/host"));
    assert_eq!(share("/../../net/host/plugin.wasm").as_deref(), Some("/net/host"));
    assert_eq!(share("/home/me/./project/../../net/host/plugin.wasm"), None);
    // the file system is case insensitive
    assert_eq!(share("/NET/Host/plugin.wasm").as_deref(), Some("/net/host"));
    assert_eq!(share("/home/me/project/../../../Net/host/plugin.wasm").as_deref(), Some("/net/host"));
    // a symlink to it
    environment.symlink("/net", "/home/me/project/net_link").unwrap();
    assert_eq!(share("/home/me/project/net_link/host/plugin.wasm").as_deref(), Some("/net/host"));
    environment.symlink("/NET/host", "/home/me/project/host_link").unwrap();
    assert_eq!(share("/home/me/project/host_link/plugin.wasm").as_deref(), Some("/net/host"));
    // the path is the one it reaches the machine with
    assert_eq!(
      resolved("/home/me/project/host_link/plugin.wasm").as_deref(),
      Some(Path::new("/NET/host/plugin.wasm"))
    );
    // where a parent directory on the share isn't looked up
    assert_eq!(
      resolved("/home/me/project/net_link/host/a/../plugin.wasm").as_deref(),
      Some(Path::new("/net/host/plugin.wasm"))
    );
    assert_eq!(resolved("/net/host/plugin.wasm").as_deref(), Some(Path::new("/net/host/plugin.wasm")));
    // a parent directory after a symlink is the one of what it points at
    environment.symlink("/a/b", "/home/me/project/b_link").unwrap();
    assert_eq!(share("/home/me/project/b_link/../../net/host/plugin.wasm").as_deref(), Some("/net/host"));
    assert_eq!(share("/home/me/project/b_link/../plugin.wasm"), None);
    // a symlink loop isn't followed forever
    environment.symlink("/home/me/project/loop_b", "/home/me/project/loop_a").unwrap();
    environment.symlink("/home/me/project/loop_a", "/home/me/project/loop_b").unwrap();
    assert!(share("/home/me/project/loop_a/plugin.wasm").is_some());
    // only on macOS
    assert!(resolve_network_share(&environment, Path::new("/net/host/plugin.wasm"), WINDOWS, None).is_none());
  }

  #[test]
  fn should_not_leave_a_share_without_asking() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/home/me").unwrap();
    let share = |path: &str| resolve_network_share(&environment, Path::new(path), MACOS, None).map(|network_path| network_path.share);
    // a parent directory that stays on the share
    assert_eq!(share("/net/host/proj/../shared/plugin.wasm").as_deref(), Some("/net/host"));
    // but one that leaves it may reach another machine, which nothing is the same as
    let left_share = share("/net/host/proj/../../evil/plugin.wasm").unwrap();
    assert_ne!(left_share, "/net/host");
    assert_ne!(left_share, "/net/evil");
    // even when it ends up on this one, because the share's machine was contacted
    assert!(share("/net/host/../../home/me/plugin.wasm").is_some());

    let policy = DownloadPolicy::for_editor(&environment);
    let DownloadPolicy::TrustedDomains(downloads) = &policy else {
      unreachable!();
    };
    let ensure_allows = |path: &str, base: Option<&str>| downloads.ensure_allows_path(&environment, Path::new(path), base.map(Path::new), MACOS);
    // a config file on the share may use what's on it, but not what's on another machine
    assert!(ensure_allows("/net/host/proj/../shared/plugin.wasm", Some("/net/host/proj")).is_ok());
    let err = ensure_allows("/net/host/proj/../../evil/plugin.wasm", Some("/net/host/proj")).unwrap_err();
    assert!(err.download.contains("evil"), "{}", err.download);
    // allowing a path ending with a slash doesn't allow what's under it
    policy.allow(&["/net/host/share/".to_string()]);
    assert!(ensure_allows("/net/host/share/../../evil/plugin.wasm", None).is_err());
    assert!(ensure_allows("/net/host/share/plugin.wasm", None).is_err());
  }

  #[cfg(windows)]
  #[test]
  fn should_not_leave_the_root_of_a_unc_path() {
    let environment = TestEnvironment::new();
    let share = |path: &str| resolve_network_share(&environment, Path::new(path), WINDOWS, None).map(|network_path| network_path.share);
    assert_eq!(share(r"\\server\share\..\..\other\plugin.wasm").as_deref(), Some(r"server\share"));
    assert_eq!(share(r"\\server\share\a\..\plugin.wasm").as_deref(), Some(r"server\share"));
  }

  #[cfg(windows)]
  #[test]
  fn should_let_a_parent_directory_remove_the_device_of_a_device_path_like_win32() {
    let environment = TestEnvironment::new();
    let resolve = |path: &str| resolve_network_share(&environment, Path::new(path), WINDOWS, None).map(|network_path| network_path.resolved);
    // the root is `\\.\`, so this reads a UNC share
    for path in [
      r"\\.\Volume{x}\..\UNC\h\s\p.wasm",
      r"\\.\Volume{x}/../UNC/h/s/p.wasm",
      r"//./Volume{x}/../UNC/h/s/p.wasm",
      r"\\.\Volume{x}\dir\.\..\..\..\UNC\h\s\p.wasm",
      r"\\.\C:\..\UNC\h\s\p.wasm",
      r"\\.\UNC\other\s\..\..\h\s\p.wasm",
    ] {
      assert_eq!(resolve(path), Some(PathBuf::from(r"\\.\UNC\h\s\p.wasm")), "{}", path);
    }
    assert_eq!(normalize_lexically(Path::new(r"\\.\Volume{x}\a\.\..\b")), Path::new(r"\\.\Volume{x}\b"));
    assert_eq!(normalize_lexically(Path::new(r"\\.\Volume{x}\a\..")), Path::new(r"\\.\Volume{x}\"));
    assert_eq!(normalize_lexically(Path::new(r"\\.\UNC\h\s\..\..\..\..\x")), Path::new(r"\\.\x"));
    // and a device path is on another machine even when its device is a volume or a drive
    assert!(resolve(r"\\.\Volume{x}\p.wasm").is_some());
    assert!(resolve(r"\\.\C:\p.wasm").is_some());
    // which is shown with what Win32 reads, also to a config file on the volume
    let policy = DownloadPolicy::for_editor(&environment);
    let path = Path::new(r"\\.\Volume{x}\..\UNC\h\s\p.wasm");
    for base in [r"C:\project", r"\\.\Volume{x}\project\dprint.json", r"\\.\UNC\h\s\dprint.json"] {
      let err = policy.ensure_allows_path(&environment, path, Some(Path::new(base))).unwrap_err();
      assert_eq!(err.download, r"\\.\Volume{x}\..\UNC\h\s\p.wasm -> \\.\UNC\h\s\p.wasm", "{}", base);
    }
  }

  #[cfg(windows)]
  #[test]
  fn should_not_follow_a_parent_directory_that_leaves_the_root_of_a_verbatim_path_on_windows() {
    let environment = TestEnvironment::new();
    let resolve = |path: &str| resolve_network_share(&environment, Path::new(path), WINDOWS, None);
    // the system doesn't apply it, so where it's read from isn't known
    for path in [
      r"\\?\Volume{x}\..\UNC\h\s\p.wasm",
      r"\\?\C:\..\UNC\h\s\p.wasm",
      r"\\?\UNC\h\s\..\..\other\s\p.wasm",
    ] {
      let network_path = resolve(path).unwrap_or_else(|| panic!("{}", path));
      assert_eq!(network_path.resolved, Path::new(path));
      assert_eq!(network_path.share, path.to_lowercase());
    }
    // but one that stays in it is followed like before
    let network_path = resolve(r"\\?\UNC\h\s\a\..\p.wasm").unwrap();
    assert_eq!(network_path.share, r"h\s");
    assert_eq!(network_path.resolved, Path::new(r"\\?\UNC\h\s\p.wasm"));
    assert!(resolve(r"\\?\Volume{x}\a\..\p.wasm").is_none());
    // and a config file on the share can't use what leaves it
    let policy = DownloadPolicy::for_editor(&environment);
    let base = Some(Path::new(r"\\?\UNC\h\s\project\dprint.json"));
    assert!(policy.ensure_allows_path(&environment, Path::new(r"\\?\UNC\h\s\project\p.wasm"), base).is_ok());
    let err = policy
      .ensure_allows_path(&environment, Path::new(r"\\?\UNC\h\s\..\..\other\s\p.wasm"), base)
      .unwrap_err();
    assert_eq!(err.download, r"\\?\UNC\h\s\..\..\other\s\p.wasm");
    // the root of a drive stays the root when Win32 applies `..`
    assert!(resolve(r"C:\..\..\p.wasm").is_none());
  }

  #[test]
  fn should_apply_parent_directories_to_the_text_first_like_windows() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/project").unwrap();
    environment.mk_dir_all("/real/deep").unwrap();
    environment.symlink("/real/deep", "/project/link").unwrap();
    environment.symlink("/net/attacker/share", "/project/remote").unwrap();
    let share = |path: &str, rules: PathRules| resolve_network_share(&environment, Path::new(path), rules, None).map(|network_path| network_path.share);
    // `link\..` is the project's directory, so this reads `remote`
    assert_eq!(share("/project/link/../remote/plugin.wasm", LEXICAL_NET).as_deref(), Some("/net/attacker"));
    // which is `/real/remote` when `..` is applied after following `link`
    assert_eq!(share("/project/link/../remote/plugin.wasm", MACOS), None);
    // the same goes for the `..` in the target of a symlink (the test environment
    // resolves a relative target itself, so it's checked on its own)
    assert_eq!(
      get_symlink_target_path(Path::new("/project"), Path::new("link/../remote"), LEXICAL_NET),
      Path::new("/project/remote")
    );
    assert_eq!(
      get_symlink_target_path(Path::new("/project"), Path::new("link/../remote"), MACOS),
      Path::new("/project/link/../remote")
    );
    assert_eq!(
      get_symlink_target_path(Path::new("/project"), Path::new("/a/./b/../../../c"), LEXICAL_NET),
      Path::new("/c")
    );
    // where the parent of the root is the root
    assert_eq!(
      share("/../../project/link/../remote/plugin.wasm", LEXICAL_NET).as_deref(),
      Some("/net/attacker")
    );
    // and the walks of ancestor directories do the same
    let policy = DownloadPolicy::for_editor(&environment);
    let ancestor_paths = AncestorPaths::new(&policy, Path::new("/project/link"), LEXICAL_NET);
    let err = ancestor_paths
      .ensure_allows(&environment, Path::new("/project/link/../remote/.npmrc"), None)
      .unwrap_err();
    assert_eq!(err.download.replace('\\', "/"), "/project/link/../remote/.npmrc -> /net/attacker/share/.npmrc");
    assert!(ancestor_paths.ensure_allows(&environment, Path::new("/project/link/.npmrc"), None).is_ok());
  }

  #[cfg(windows)]
  #[test]
  fn should_apply_parent_directories_to_the_text_first_on_windows() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/project").unwrap();
    environment.mk_dir_all("/real/deep").unwrap();
    environment.symlink("/real/deep", "/project/link").unwrap();
    environment.symlink(r"\\attacker.example\share", "/project/x").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let base = Some(Path::new("/project"));
    let err = policy
      .ensure_allows_path(&environment, Path::new(r"\project\link\..\x\plugin.wasm"), base)
      .unwrap_err();
    assert_eq!(err.download, r"\project\link\..\x\plugin.wasm -> \\attacker.example\share\plugin.wasm");
    // and to the target of a relative symlink
    assert_eq!(
      get_symlink_target_path(Path::new(r"C:\project"), Path::new(r"link\..\x"), PATH_RULES),
      Path::new(r"C:\project\x")
    );
    // but not to a verbatim one, which is used as it is
    assert_eq!(
      get_symlink_target_path(Path::new(r"C:\project"), Path::new(r"\\?\C:\a\..\b"), PATH_RULES),
      Path::new(r"\\?\C:\a\..\b")
    );
  }

  #[test]
  fn should_follow_the_share_of_the_file_a_path_is_relative_to() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/net/fs/home/u/proj").unwrap();
    environment.mk_dir_all("/local/dir").unwrap();
    environment.symlink("/net/attacker/x", "/net/fs/home/u/proj/tools").unwrap();
    environment.symlink("../shared", "/net/fs/home/u/proj/shared_link").unwrap();
    environment.symlink("/local/dir", "/net/fs/home/u/proj/local_link").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let DownloadPolicy::TrustedDomains(downloads) = &policy else {
      unreachable!();
    };
    let base = Path::new("/net/fs/home/u/proj");
    let ensure_allows = |path: &str| downloads.ensure_allows_path(&environment, Path::new(path), Some(base), MACOS);
    // a symlink on the share is followed because its machine is already being read
    let err = ensure_allows("/net/fs/home/u/proj/tools/plugin.wasm").unwrap_err();
    assert_eq!(
      err.download.replace('\\', "/"),
      "/net/fs/home/u/proj/tools/plugin.wasm -> /net/attacker/x/plugin.wasm"
    );
    assert!(ensure_allows("/net/fs/home/u/proj/plugin.wasm").is_ok());
    assert!(ensure_allows("/net/fs/home/u/proj/shared_link/plugin.wasm").is_ok());
    assert!(ensure_allows("/net/fs/home/u/proj/local_link/plugin.wasm").is_ok());
    // and so do the walks of ancestor directories
    let ancestor_paths = AncestorPaths::new(&policy, base, MACOS);
    assert!(ancestor_paths.ensure_allows(&environment, &base.join("tools/.npmrc"), Some(base)).is_err());
    assert!(ancestor_paths.ensure_allows(&environment, Path::new("/net/fs/home/.npmrc"), Some(base)).is_ok());
    // but what's on another share isn't looked up
    let count = environment.path_kind_count();
    assert!(ensure_allows("/net/other/a/b/plugin.wasm").is_err());
    // only `/net`
    assert_eq!(environment.path_kind_count(), count + 1);
  }

  #[test]
  fn should_follow_a_share_once_reading_it_is_allowed() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/project/node_modules").unwrap();
    environment.mk_dir_all("/net/host_a/share/pkg").unwrap();
    environment.write_file("/net/host_a/share/pkg/other.wasm", "").unwrap();
    environment.symlink("/net/host_a/share/pkg", "/project/node_modules/pkg").unwrap();
    // ex. the NFS client of macOS follows it when it's read, which has the automounter contact `host_b`
    environment.symlink("/net/host_b/x/plugin.wasm", "/net/host_a/share/pkg/plugin.wasm").unwrap();
    environment.symlink("/net/host_b/x/.npmrc", "/net/host_a/share/pkg/.npmrc").unwrap();
    environment.symlink("/net/host_a/share/pkg2", "/net/host_a/share/pkg/same_share_link").unwrap();
    environment.symlink("/net/host_b/x", "/net/host_a/share/pkg2").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let DownloadPolicy::TrustedDomains(downloads) = &policy else {
      unreachable!();
    };
    let base = Path::new("/project");
    let ensure_allows = |path: &str| downloads.ensure_allows_path(&environment, Path::new(path), Some(base), MACOS);
    let download = |err: UntrustedDownloadError| err.download.replace('\\', "/");
    // what's on the share isn't looked up before it's allowed
    let err = ensure_allows("/project/node_modules/pkg/plugin.wasm").unwrap_err();
    assert_eq!(download(err), "/project/node_modules/pkg/plugin.wasm -> /net/host_a/share/pkg/plugin.wasm");
    let err = ensure_allows("/project/node_modules/pkg").unwrap_err();
    policy.allow(&[err.download]);
    assert!(ensure_allows("/project/node_modules/pkg/other.wasm").is_ok());
    // but once it is, a symlink on it that reaches another machine needs to be allowed too
    let err = ensure_allows("/project/node_modules/pkg/plugin.wasm").unwrap_err();
    assert_eq!(download(err), "/project/node_modules/pkg/plugin.wasm -> /net/host_b/x/plugin.wasm");
    let err = ensure_allows("/project/node_modules/pkg/same_share_link/plugin.wasm").unwrap_err();
    assert_eq!(
      download(err),
      "/project/node_modules/pkg/same_share_link/plugin.wasm -> /net/host_b/x/plugin.wasm"
    );
    // and so do the walks of ancestor directories
    let ancestor_paths = AncestorPaths::new(&policy, Path::new("/project/node_modules/pkg"), MACOS);
    let err = ancestor_paths
      .ensure_allows(&environment, Path::new("/project/node_modules/pkg/.npmrc"), Some(base))
      .unwrap_err();
    assert_eq!(download(err), "/project/node_modules/pkg/.npmrc -> /net/host_b/x/.npmrc");
    assert!(
      ancestor_paths
        .ensure_allows(&environment, Path::new("/project/node_modules/pkg/other.wasm"), Some(base))
        .is_ok()
    );
    // which is then allowed
    let err = ensure_allows("/project/node_modules/pkg/plugin.wasm").unwrap_err();
    policy.allow(&[err.download]);
    assert!(ensure_allows("/project/node_modules/pkg/plugin.wasm").is_ok());
  }

  #[test]
  fn should_stop_at_the_root_of_a_share_when_going_up_ancestor_directories() {
    let ancestors = |dir: &'static str, is_net_automounted: bool| {
      std::iter::successors(Some(Path::new(dir)), move |dir| get_parent_on_share_with(dir, is_net_automounted)).collect::<Vec<_>>()
    };
    // `/net/.npmrc` would be a host named `.npmrc`
    assert_eq!(ancestors("/net/fs/proj", true), [Path::new("/net/fs/proj"), Path::new("/net/fs")]);
    assert_eq!(
      ancestors("/System/Volumes/Data/net/fs/proj", true),
      [Path::new("/System/Volumes/Data/net/fs/proj"), Path::new("/System/Volumes/Data/net/fs")]
    );
    assert_eq!(ancestors("/home/me/proj", true).len(), 4);
    // only on macOS
    assert_eq!(ancestors("/net/fs/proj", false).len(), 4);
  }

  #[test]
  fn should_only_stop_at_a_share_in_net_when_anything_may_be_read_when_net_is_an_automounters() {
    /// The paths of Linux, where `/net` is only an automounter's when it's set up.
    const LINUX: PathRules = PathRules {
      is_net_automounted: true,
      is_net_always_automounted: false,
      is_parent_lexical: false,
      has_windows_names: false,
    };
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/net/work/proj").unwrap();
    let ancestors = |policy: &DownloadPolicy, rules: PathRules| {
      AncestorPaths::new(policy, Path::new("/net/work/proj"), rules)
        .ancestors(&environment)
        .into_iter()
        .map(|dir| dir.to_path_buf())
        .collect::<Vec<_>>()
    };
    let net_share = [PathBuf::from("/net/work/proj"), PathBuf::from("/net/work")];
    // an ordinary `/net` directory on Linux
    assert_eq!(ancestors(&DownloadPolicy::All, LINUX).len(), 4);
    assert_eq!(ancestors(&DownloadPolicy::All, MACOS), net_share);
    // but the editor treats it as a share either way
    let policy = DownloadPolicy::for_editor(&environment);
    assert_eq!(ancestors(&policy, LINUX), net_share);
    // and an automounter's `/net` on Linux
    environment.add_automount_dir("/net");
    assert_eq!(ancestors(&DownloadPolicy::All, LINUX), net_share);
  }

  #[test]
  fn should_treat_net_on_the_data_volume_like_net() {
    let share = |path: &str| get_network_share(Path::new(path), true);
    assert_eq!(share("/System/Volumes/Data/net/Host/plugin.wasm").as_deref(), Some("/net/host"));
    assert_eq!(share("/system/volumes/data/NET/host").as_deref(), Some("/net/host"));
    assert_eq!(share("/System/Volumes/Data/net"), None);
    assert_eq!(share("/System/Volumes/Data/home/plugin.wasm"), None);
    assert_eq!(share("/System/Library/net/host"), None);
    assert_eq!(get_network_share(Path::new("/System/Volumes/Data/net/host"), false), None);

    // which is the same share as `/net`
    let environment = TestEnvironment::new();
    let policy = DownloadPolicy::for_editor(&environment);
    let DownloadPolicy::TrustedDomains(downloads) = &policy else {
      unreachable!();
    };
    let ensure_allows = |path: &str, base: &str| downloads.ensure_allows_path(&environment, Path::new(path), Some(Path::new(base)), MACOS);
    assert!(ensure_allows("/System/Volumes/Data/net/host/plugin.wasm", "/net/host/proj").is_ok());
    assert!(ensure_allows("/System/Volumes/Data/net/other/plugin.wasm", "/net/host/proj").is_err());
    assert!(ensure_allows("/System/Volumes/Data/net/other/plugin.wasm", "/home/me/proj").is_err());
  }

  #[test]
  fn should_check_the_paths_in_ancestor_directories_like_each_path() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/a/b/c/d/e/f/g/h").unwrap();
    environment.mk_dir_all("/x").unwrap();
    environment.symlink("/net", "/a/b/net_link").unwrap();
    environment.symlink("/net/host/npmrc", "/a/b/c/.npmrc").unwrap();
    environment.symlink("/x", "/a/b/c/d/x_link").unwrap();
    environment.symlink("/net/other/dir", "/x/remote").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let dir = Path::new("/a/b/c/d/e/f/g/h");
    let ancestor_paths = AncestorPaths::new(&policy, dir, MACOS);
    let DownloadPolicy::TrustedDomains(downloads) = &policy else {
      unreachable!();
    };
    let ensure_allows_each = |path: &Path| {
      let network_path = resolve_network_share(&environment, path, MACOS, None);
      downloads
        .ensure_allows_network_path(path, network_path, get_network_share(dir, true).as_deref(), |allowed_shares| {
          resolve_network_share(&environment, path, MACOS, allowed_shares.to_vec())
        })
        .map_err(|err| err.download)
    };
    let mut paths = dir.ancestors().map(|dir| dir.join(".npmrc")).collect::<Vec<_>>();
    paths.extend([
      dir.join("../../../../x_link/remote/plugin.wasm"),
      Path::new("/a/b/c/d/x_link/remote/plugin.wasm").to_path_buf(),
      Path::new("/a/b/net_link/host/plugin.wasm").to_path_buf(),
      Path::new("/a/b/c/d/x_link/local.wasm").to_path_buf(),
    ]);
    for path in &paths {
      assert_eq!(
        ancestor_paths.ensure_allows(&environment, path, Some(dir)).map_err(|err| err.download),
        ensure_allows_each(path),
        "{}",
        path.display()
      );
    }
    assert!(ancestor_paths.ensure_allows(&environment, Path::new("/a/b/c/.npmrc"), Some(dir)).is_err());
    assert!(ancestor_paths.ensure_allows(&environment, Path::new("/a/b/.npmrc"), Some(dir)).is_ok());

    // the directories are only followed once, so each path only looks up what's after its directory
    let ancestor_paths = AncestorPaths::new(&policy, dir, MACOS);
    let count = environment.path_kind_count();
    for dir in dir.ancestors() {
      ancestor_paths.ensure_allows(&environment, &dir.join(".npmrc"), Some(dir)).ok();
    }
    let ancestor_count = dir.ancestors().count();
    // the directory's components and then the `.npmrc` file of each one
    assert!(
      environment.path_kind_count() - count <= ancestor_count * 2,
      "{}",
      environment.path_kind_count() - count
    );

    // and nothing is looked up when anything may be read
    let count = environment.path_kind_count();
    let ancestor_paths = DownloadPolicy::All.ancestor_paths(dir);
    assert!(ancestor_paths.ensure_allows(&environment, Path::new("/a/b/c/.npmrc"), None).is_ok());
    assert_eq!(environment.path_kind_count(), count);
  }

  #[cfg(windows)]
  #[test]
  fn should_follow_the_file_of_an_alternate_data_stream_on_windows() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/project").unwrap();
    environment.write_file("/project/file.txt", "").unwrap();
    environment.symlink(r"\\server\share\dir", "/project/link").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let base = Some(Path::new("/project"));
    let ensure_allows = |path: &str| policy.ensure_allows_path(&environment, Path::new(path), base).map_err(|err| err.download);
    // looking up the stream of a symlink fails although reading it follows the symlink,
    // so the stream is of what the symlink points at
    let resolved = Path::new(r"\\server\share\dir:plugin.wasm");
    assert_eq!(
      ensure_allows("/project/link:plugin.wasm"),
      Err(format!("{} -> {}", Path::new("/project/link:plugin.wasm").display(), resolved.display()))
    );
    assert!(ensure_allows("/project/link::$DATA").is_err());
    assert!(ensure_allows("/project/link:plugin.wasm:$DATA").is_err());
    assert!(ensure_allows("/project/link/sub:x").is_err());
    // the stream of a local file or directory, or of a file that doesn't exist
    assert!(ensure_allows("/project/file.txt:stream").is_ok());
    assert!(ensure_allows("/project:stream").is_ok());
    assert!(ensure_allows("/project/missing:stream").is_ok());
    // but not one whose file can't be looked up although the stream can
    environment.write_file("/project/unknown:stream", "").unwrap();
    assert!(ensure_allows("/project/unknown:stream").is_err());
  }

  #[cfg(windows)]
  #[test]
  fn should_follow_a_real_symlink_with_an_alternate_data_stream_on_windows() {
    use crate::environment::RealEnvironment;
    use crate::environment::RealEnvironmentOptions;

    let environment = RealEnvironment::new(RealEnvironmentOptions {
      log_level: crate::utils::LogLevel::Info,
      is_stdout_machine_readable: false,
    })
    .unwrap();
    let temp_dir = tempfile::tempdir().unwrap();
    let link_path = temp_dir.path().join("link");
    // creating a symlink needs developer mode or elevation, and the target isn't contacted
    if std::os::windows::fs::symlink_file(r"\\dprint-test.invalid\share\target.txt", &link_path).is_err() {
      return;
    }
    let policy = DownloadPolicy::for_editor(&environment);
    let err = policy
      .ensure_allows_path(&environment, &temp_dir.path().join("link:plugin.wasm"), Some(temp_dir.path()))
      .unwrap_err();
    assert!(err.download.contains("dprint-test.invalid"), "{}", err.download);
    assert!(err.download.ends_with(r"\target.txt:plugin.wasm"), "{}", err.download);
    assert!(policy.ensure_allows_path(&environment, &link_path, Some(temp_dir.path())).is_err());
  }

  #[test]
  fn should_not_look_up_a_windows_name_that_win32_changes() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/project/dir").unwrap();
    let share = |path: &str| resolve_network_share(&environment, Path::new(path), WINDOWS, None).map(|network_path| network_path.share);
    assert_eq!(share("/project/dir/plugin.wasm"), None);
    // a trailing dot or space isn't always removed the same way when it's looked up as when it's read
    assert!(share("/project/dir./plugin.wasm").is_some());
    assert!(share("/project/dir /plugin.wasm").is_some());
    assert!(share("/project/plugin.wasm.").is_some());
    assert!(share("/project/.../plugin.wasm").is_some());
    // and a device may be on another machine (ex. a printer)
    for name in ["LPT1", "lpt1", "COM9", "nul.txt", "CON ", "conout$", "AUX:stream", "LPT\u{b9}"] {
      assert!(share(&format!("/project/{}", name)).is_some(), "{}", name);
    }
    for name in ["LPT", "COM10", "console", "nullable.txt", "lpt1x"] {
      assert_eq!(share(&format!("/project/{}", name)), None, "{}", name);
    }
    // only on windows
    assert!(resolve_network_share(&environment, Path::new("/project/LPT1"), MACOS, None).is_none());
  }

  #[cfg(windows)]
  #[test]
  fn should_not_change_a_name_in_a_verbatim_path_on_windows() {
    assert!(matches!(WindowsName::new(Path::new(r"\\?\C:\project"), OsStr::new("LPT1")), WindowsName::Plain));
    assert!(matches!(WindowsName::new(Path::new(r"\\?\C:\project"), OsStr::new("dir.")), WindowsName::Plain));
    assert!(matches!(WindowsName::new(Path::new(r"C:\project"), OsStr::new("dir.")), WindowsName::Changed));
    assert!(matches!(
      WindowsName::new(Path::new(r"\\?\C:\project"), OsStr::new("link:stream")),
      WindowsName::Stream("link", ":stream")
    ));
  }

  #[cfg(windows)]
  #[test]
  fn should_treat_a_volume_like_a_drive_on_windows() {
    let environment = TestEnvironment::new();
    let policy = DownloadPolicy::for_editor(&environment);
    let volume = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}";
    let dir = PathBuf::from(format!(r"{}\repo\src", volume));
    assert_eq!(get_network_share(&dir, false), None);
    // but not one in a device path, whose `..` Win32 applies to the volume too
    assert!(get_network_share(Path::new(r"\\.\Volume{01234567-89ab-cdef-0123-456789abcdef}\repo"), false).is_some());
    assert!(policy.ensure_allows_path(&environment, &dir.join("plugin.wasm"), None).is_ok());
    // the ancestors of a path on a volume or with a prefix whose share isn't known aren't stopped at the first directory
    let ancestors = |dir: &Path| policy.ancestor_paths(dir).ancestors(&environment).len();
    assert_eq!(ancestors(&dir), 3);
    assert_eq!(ancestors(Path::new(r"\\?\GLOBALROOT\Device\Mup\server\share\proj")), 6);
    // but what's in them is on another machine
    let globalroot_dir = Path::new(r"\\?\GLOBALROOT\Device\Mup\server\share\proj");
    let ancestor_paths = policy.ancestor_paths(globalroot_dir);
    assert!(
      ancestor_paths
        .ensure_allows(&environment, &globalroot_dir.join(".npmrc"), Some(globalroot_dir))
        .is_err()
    );
    // and a UNC path still stops at the root of its share
    assert_eq!(ancestors(Path::new(r"\\server\share\proj")), 2);
  }

  #[test]
  fn should_fold_the_case_of_any_letter_in_net_like_the_file_system() {
    let share = |path: &str| get_network_share(Path::new(path), true);
    // `ſ` (U+017F) folds to `s` on a case insensitive file system
    assert_eq!(share("/\u{17f}ystem/Volumes/Data/net/host/plugin.wasm").as_deref(), Some("/net/host"));
    assert_eq!(share("/System/Volume\u{17f}/Data/net/host").as_deref(), Some("/net/host"));
    assert_eq!(share("/system/volumes/data/NET/host").as_deref(), Some("/net/host"));
    assert_eq!(share("/\u{17f}ystem/Volumes/Data/other/host"), None);
  }

  #[test]
  fn should_treat_what_is_in_an_automount_directory_as_on_another_machine() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/auto/fs/home/proj").unwrap();
    environment.mk_dir_all("/local/proj").unwrap();
    environment.add_automount_dir("/auto");
    environment.symlink("/auto/attacker/x", "/auto/fs/home/proj/tools").unwrap();
    environment.symlink("/auto/fs/home", "/local/proj/home_link").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let DownloadPolicy::TrustedDomains(downloads) = &policy else {
      unreachable!();
    };
    let ensure_allows = |path: &str, base: &str| {
      downloads
        .ensure_allows_path(&environment, Path::new(path), Some(Path::new(base)), MACOS)
        .map_err(|err| err.download.replace('\\', "/"))
    };
    // what's in it isn't looked up because that may mount it from another machine
    let count = environment.path_kind_count();
    assert!(ensure_allows("/auto/other/plugin.wasm", "/local/proj").is_err());
    // only `/auto`, and the directory of the config file to see whether it's on a share
    assert_eq!(environment.path_kind_count(), count + 3);
    // and a symlink to it is followed to it
    assert_eq!(
      ensure_allows("/local/proj/home_link/plugin.wasm", "/local/proj"),
      Err("/local/proj/home_link/plugin.wasm -> /auto/fs/home/plugin.wasm".to_string())
    );
    // a config file on the share may use what's on it, which is followed like a local path
    let base = "/auto/fs/home/proj";
    assert_eq!(ensure_allows("/auto/fs/home/proj/plugin.wasm", base), Ok(()));
    assert_eq!(ensure_allows("/auto/fs/home/proj/../plugin.wasm", base), Ok(()));
    assert_eq!(
      ensure_allows("/auto/fs/home/proj/tools/plugin.wasm", base),
      Err("/auto/fs/home/proj/tools/plugin.wasm -> /auto/attacker/x/plugin.wasm".to_string())
    );
    assert!(ensure_allows("/auto/fs/../other/plugin.wasm", base).is_err());
    // but not when it was reached through a symlink, like a share in the text of a path
    assert!(ensure_allows("/auto/fs/home/plugin.wasm", "/local/proj/home_link").is_err());

    // the walks of ancestor directories do the same and stop at the root of the share
    let ancestor_paths = AncestorPaths::new(&policy, Path::new(base), MACOS);
    assert_eq!(
      ancestor_paths.ancestors(&environment),
      [Path::new("/auto/fs/home/proj"), Path::new("/auto/fs/home"), Path::new("/auto/fs")]
    );
    assert!(
      ancestor_paths
        .ensure_allows(&environment, Path::new("/auto/fs/.npmrc"), Some(Path::new(base)))
        .is_ok()
    );
    assert!(
      ancestor_paths
        .ensure_allows(&environment, Path::new("/auto/fs/home/proj/tools/.npmrc"), Some(Path::new(base)))
        .is_err()
    );
    // and nothing above it is followed
    assert!(
      ancestor_paths
        .ensure_allows(&environment, Path::new("/auto/.npmrc"), Some(Path::new(base)))
        .is_err()
    );
    let local_ancestor_paths = AncestorPaths::new(&policy, Path::new("/local/proj"), MACOS);
    assert_eq!(local_ancestor_paths.ancestors(&environment).len(), 3);
  }

  #[test]
  fn should_treat_what_is_in_an_automount_point_as_on_another_machine() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/mnt/data/dir").unwrap();
    environment.mk_dir_all("/project").unwrap();
    environment.add_automount_point("/mnt/data");
    environment.symlink("/mnt/data", "/project/tools").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let DownloadPolicy::TrustedDomains(downloads) = &policy else {
      unreachable!();
    };
    let ensure_allows = |path: &str, base: &str| {
      downloads
        .ensure_allows_path(&environment, Path::new(path), Some(Path::new(base)), MACOS)
        .map_err(|err| err.download.replace('\\', "/"))
    };
    // what's in it isn't looked up because that may mount it from another machine
    let count = environment.path_kind_count();
    assert_eq!(
      ensure_allows("/project/tools/dir/plugin.wasm", "/project"),
      Err("/project/tools/dir/plugin.wasm -> /mnt/data/dir/plugin.wasm".to_string())
    );
    // only `project`, `tools`, `mnt` and `data`, and the directory of the config file to see whether it's on a share
    assert_eq!(environment.path_kind_count(), count + 5);
    // a config file on the share may use what's on it
    assert_eq!(ensure_allows("/mnt/data/dir/plugin.wasm", "/mnt/data/dir"), Ok(()));
    // and leaving it with `..` reaches what is local again
    assert_eq!(ensure_allows("/mnt/data/../plugin.wasm", "/mnt/data/dir"), Ok(()));
    // but not when it was reached through a symlink, like a share in the text of a path
    assert!(ensure_allows("/mnt/data/dir/plugin.wasm", "/project/tools/dir").is_err());
    // and the walks of ancestor directories stop at it
    let ancestor_paths = AncestorPaths::new(&policy, Path::new("/mnt/data/dir"), MACOS);
    assert_eq!(ancestor_paths.ancestors(&environment), [Path::new("/mnt/data/dir"), Path::new("/mnt/data")]);
  }

  #[test]
  fn should_say_whether_a_path_that_was_checked_may_exist() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/a/b").unwrap();
    environment.write_file("/a/b/file.txt", "").unwrap();
    environment.symlink("/a/b/file.txt", "/a/b/link").unwrap();
    environment.symlink("/a/b/missing.txt", "/a/b/broken_link").unwrap();
    let policy = DownloadPolicy::for_editor(&environment);
    let ancestor_paths = AncestorPaths::new(&policy, Path::new("/a/b"), MACOS);
    let may_exist = |path: &str| ancestor_paths.ensure_allows_existing(&environment, Path::new(path), None).unwrap();
    assert!(may_exist("/a/b/file.txt"));
    assert!(may_exist("/a/b/link"));
    assert!(may_exist("/a/b"));
    assert!(may_exist("/a"));
    assert!(!may_exist("/a/b/missing.txt"));
    assert!(!may_exist("/a/b/broken_link"));
    assert!(!may_exist("/a/missing/file.txt"));
    // nothing is looked up when anything may be read
    assert!(
      DownloadPolicy::All
        .ancestor_paths(Path::new("/a/b"))
        .ensure_allows_existing(&environment, Path::new("/a/b/missing.txt"), None)
        .unwrap()
    );
  }

  #[test]
  fn should_only_allow_the_npmrc_proxy_that_the_user_allowed() {
    let environment = TestEnvironment::new();
    let policy = DownloadPolicy::for_editor(&environment);
    let proxy = NpmrcProxy {
      npmrc_path: "/project/.npmrc".to_string(),
      setting: "https://${TOKEN}@proxy.example".to_string(),
    };
    let through_proxy = DownloadProxy::Npmrc {
      url: "https://token@proxy.example",
      source: &proxy,
    };
    let trusted_url = url("https://registry.npmjs.org/@dprint/typescript");
    // even a trusted domain isn't downloaded through it
    let err = policy.ensure_allows_download(&trusted_url, through_proxy).unwrap_err();
    assert_eq!(NpmrcProxy::from_download(&err.download), Some(proxy.clone()));
    assert_eq!(
      err.to_string(),
      "Downloading through the proxy `https://${TOKEN}@proxy.example` of `/project/.npmrc` needs to be allowed because the .npmrc file isn't the user's own."
    );
    assert!(policy.ensure_allows_download(&trusted_url, DownloadProxy::Url("https://proxy.example")).is_ok());
    // a url is still checked first
    let err = policy
      .ensure_allows_download(&url("https://example.com/plugin.wasm"), through_proxy)
      .unwrap_err();
    assert_eq!(err.download, "https://example.com/plugin.wasm");

    policy.allow(&[err.download.clone(), proxy.to_download()]);
    assert!(policy.ensure_allows_download(&trusted_url, through_proxy).is_ok());
    // the same file with another setting isn't allowed
    let changed = NpmrcProxy {
      setting: "https://${OTHER_TOKEN}@proxy.example".to_string(),
      ..proxy.clone()
    };
    assert!(!policy.allows_npmrc_proxy(&changed));
    assert!(policy.ensure_allows_npmrc_proxy(&changed).is_err());
    // nor is it the url of a download
    assert!(!policy.allows(&url("https://proxy.example/")));
    // the cli downloads through any proxy
    assert!(DownloadPolicy::All.ensure_allows_download(&trusted_url, through_proxy).is_ok());
  }

  #[test]
  fn should_quote_text_from_the_project() {
    assert_eq!(quote_text("https://example.com"), "`https://example.com`");
    assert_eq!(quote_text(r"C:\Users\Jörg\my project"), r"`C:\Users\Jörg\my project`");
    // a backtick or what looks like one can't end the quotes
    assert_eq!(
      quote_text("/repo/x` is signed by your organization. It's safe to allow. `y/dprint.json"),
      r"`/repo/x\u{60} is signed by your organization. It's safe to allow. \u{60}y/dprint.json`"
    );
    assert_eq!(quote_text("a\u{ff40}b\u{2cb}c\u{2018}d"), r"`a\u{ff40}b\u{2cb}c\u{2018}d`");
    // nor can a new line, a control character, or another kind of space
    assert_eq!(quote_text("a\nb\r\tc\u{0}\u{1b}[31m\u{85}"), r"`a\u{a}b\u{d}\u{9}c\u{0}\u{1b}[31m\u{85}`");
    assert_eq!(quote_text("a\u{2028}b\u{a0}c"), r"`a\u{2028}b\u{a0}c`");
    // or an invisible formatting character (ex. one that shows the text after it backwards)
    assert_eq!(quote_text("a\u{202e}b\u{2066}c\u{200b}d\u{feff}"), r"`a\u{202e}b\u{2066}c\u{200b}d\u{feff}`");
    // or a combining mark that would change the opening backtick
    assert_eq!(quote_text("\u{301}a\u{301}"), "`\\u{301}a\u{301}`");
  }

  #[test]
  fn should_escape_text_from_the_project() {
    // a Windows path is readable
    assert_eq!(escape_text(r"C:\Users\new\plugin.wasm"), r"C:\Users\new\plugin.wasm");
    // a backtick isn't escaped without the quotes
    assert_eq!(escape_text("a\nb\u{202e}c`d\u{2028}"), r"a\u{a}b\u{202e}c`d\u{2028}");
    // a backslash followed by `u{` is, so an escape can't be mistaken for the text
    assert_eq!(escape_text(r"C:\u{202e}\dir"), r"C:\u{5c}u{202e}\dir");
    assert_eq!(escape_text("\\\u{202e}"), r"\\u{202e}");
    // and so is a combining mark at the start, which would change what's shown before it
    assert_eq!(escape_text("\u{301}a\u{301}"), "\\u{301}a\u{301}");
    // each escape is of a code point, so the text is known
    for text in [r"C:\new", "a\nb", r"\u{a}", "\\\n", r"\\u{5c}u{", "x\u{202e}\\u{", "a->b`"] {
      assert_eq!(unescape_text(&escape_text(text)), text);
      assert_eq!(unescape_text(&escape_text_with(text, true, true)), text);
    }
  }

  #[test]
  fn should_escape_by_the_unicode_properties_of_each_character() {
    // control characters, separators other than a plain space, and format characters (ex. the
    // Arabic number signs and the Egyptian hieroglyph and shorthand format controls)
    for c in [
      '\u{0}',
      '\u{85}',
      '\u{a0}',
      '\u{3000}',
      '\u{2029}',
      '\u{ad}',
      '\u{600}',
      '\u{605}',
      '\u{6dd}',
      '\u{70f}',
      '\u{8e2}',
      '\u{110bd}',
      '\u{110cd}',
      '\u{13430}',
      '\u{1343f}',
      '\u{1bca0}',
      '\u{1bca3}',
      '\u{fff9}',
      '\u{e0001}',
    ] {
      assert!(is_escaped_in_text(c), "{:?}", c);
    }
    // the other default ignorable code points (ex. the Hangul fillers and the variation selectors)
    for c in [
      '\u{34f}',
      '\u{115f}',
      '\u{17b4}',
      '\u{180b}',
      '\u{180f}',
      '\u{2065}',
      '\u{3164}',
      '\u{fe0f}',
      '\u{ffa0}',
      '\u{fff0}',
      '\u{e0100}',
      '\u{e0fff}',
    ] {
      assert!(is_escaped_in_text(c), "{:?}", c);
    }
    // unassigned code points, which a later version of Unicode may add as format characters, and noncharacters
    for c in ['\u{378}', '\u{2fe0}', '\u{3fffd}', '\u{e0080}', '\u{fdd0}', '\u{ffff}', '\u{10ffff}'] {
      assert!(is_escaped_in_text(c), "{:?}", c);
    }
    // a combining mark is only escaped at the start, and a private use character isn't escaped
    for c in [' ', 'a', '\u{301}', '\u{489}', '\u{4e00}', '\u{1f600}', '\u{e000}', '\u{f0000}'] {
      assert!(!is_escaped_in_text(c), "{:?}", c);
    }
    // every nonspacing and enclosing mark is a combining mark (ex. the Cyrillic enclosing marks and the Hebrew and Arabic points)
    for c in [
      '\u{300}',
      '\u{483}',
      '\u{489}',
      '\u{591}',
      '\u{5bd}',
      '\u{610}',
      '\u{64b}',
      '\u{65f}',
      '\u{20dd}',
      '\u{1e8d0}',
    ] {
      assert!(is_combining_mark(c), "{:?}", c);
      assert_eq!(escape_text(&format!("{}a", c)), format!("{}a", c.escape_unicode()));
    }
    // but not a spacing mark or a letter
    assert!(!is_combining_mark('\u{903}'));
    assert!(!is_combining_mark('a'));
  }

  #[test]
  fn should_not_change_the_escaped_text_of_a_path_download() {
    // IMPORTANT: It's part of the approval ids (see APPROVAL_ID_VERSION), so a change to what's
    // escaped (ex. updating `unicode-general-category`, which assigns code points) changes them.
    assert_eq!(
      get_path_download(Path::new("/project/a\u{378}\u{200b}`b"), Path::new("\u{301}/net/host/\u{3fffd}\u{301}")),
      "/project/a\\u{378}\\u{200b}\\u{60}b -> \\u{301}/net/host/\\u{3fffd}\u{301}"
    );
  }

  #[test]
  fn should_escape_an_arrow_in_a_path_download() {
    assert_eq!(get_path_download(Path::new("/project/a->b"), Path::new("/project/a->b")), r"/project/a-\u{3e}b");
    // so the arrow between the path and where it resolved to is the only one
    assert_eq!(
      get_path_download(Path::new("/project/link"), Path::new("/net/evil/x -> /project/plugin.wasm")),
      r"/project/link -> /net/evil/x -\u{3e} /project/plugin.wasm"
    );
  }

  #[test]
  fn should_say_that_a_path_is_read_from_another_machine() {
    let path = get_path_download(Path::new("/project/.npmrc"), Path::new("/net/corp/cfg/.npmrc"));
    assert_eq!(path, "/project/.npmrc -> /net/corp/cfg/.npmrc");
    assert!(is_path_download(&path));
    assert!(is_path_download("/net/host/plugin.wasm"));
    assert!(is_path_download(r"C:\project\plugin.wasm"));
    assert!(is_path_download(r"\\server\share\plugin.wasm"));
    assert!(!is_path_download("https://example.com/plugin.wasm"));
    let proxy = NpmrcProxy {
      npmrc_path: "/project/.npmrc".to_string(),
      setting: "https://proxy.example".to_string(),
    };
    assert!(!is_path_download(&proxy.to_download()));
    // the path in the project is the one before the arrow
    assert_eq!(get_path_download_local_path(&path), Some(PathBuf::from("/project/.npmrc")));
    assert_eq!(get_path_download_local_path("/net/host/plugin.wasm"), None);
    let escaped = get_path_download(Path::new("/project/a->b\u{202e}"), Path::new("/net/x"));
    assert_eq!(escaped, r"/project/a-\u{3e}b\u{202e} -> /net/x");
    assert_eq!(get_path_download_local_path(&escaped), Some(PathBuf::from("/project/a->b\u{202e}")));

    assert_eq!(
      UntrustedDownloadError { download: path.clone() }.to_string(),
      "Reading `/project/.npmrc -> /net/corp/cfg/.npmrc` needs to be allowed because it's on another machine."
    );
    // it isn't said to trust a domain, which can't trust a path
    let url = "https://example.com/plugin.wasm".to_string();
    assert_eq!(
      get_untrusted_download_message(&[url, path, "/net/host/plugin.wasm".to_string()], None),
      concat!(
        "dprint needs to read `/project/.npmrc -> /net/corp/cfg/.npmrc`, `/net/host/plugin.wasm`, which are on another machine. ",
        "Only allow it when you trust the configuration file because reading from the machine connects to it, ",
        "which can send it information (ex. that the project was opened, or your credentials for the machine). ",
        "An .npmrc file sets where npm packages are downloaded from (ex. a company's registry for a scope), so npm packages ",
        "aren't resolved until it's read. ",
        "dprint needs to download `https://example.com/plugin.wasm` from domains that aren't trusted. ",
        "Only download them when you trust the configuration file because a download can send information to the domain. ",
        "Add a domain to the editor.trustedDomains setting in dprint's settings file to trust it."
      )
    );
    let message = get_untrusted_download_message(&["/net/host/plugin.wasm".to_string()], None);
    assert!(
      message.starts_with("dprint needs to read `/net/host/plugin.wasm`, which is on another machine."),
      "{}",
      message
    );
    assert!(!message.contains(".npmrc") && !message.contains("trustedDomains"), "{}", message);
  }

  #[test]
  fn should_say_what_needs_to_be_allowed() {
    let proxy = NpmrcProxy {
      npmrc_path: "/project/.npmrc".to_string(),
      setting: "https://${TOKEN}@proxy.example".to_string(),
    };
    let url = "https://example.com/plugin.wasm".to_string();
    let message = get_untrusted_download_message(std::slice::from_ref(&url), None);
    assert!(
      message.starts_with("dprint needs to download `https://example.com/plugin.wasm` from domains that aren't trusted."),
      "{}",
      message
    );
    assert!(
      message.ends_with("Add a domain to the editor.trustedDomains setting in dprint's settings file to trust it."),
      "{}",
      message
    );
    // what the user already did isn't suggested when the settings file isn't used
    let message = get_untrusted_download_message(std::slice::from_ref(&url), Some("The settings file isn't used."));
    assert!(
      message.ends_with("send information to the domain. The settings file isn't used."),
      "{}",
      message
    );
    // the proxy is said first, so a long url doesn't push it past where an editor cuts off the message
    let message = get_untrusted_download_message(&[url.clone(), proxy.to_download()], None);
    assert!(
      message.starts_with(concat!(
        "The proxy `https://${TOKEN}@proxy.example` that `/project/.npmrc` sets isn't trusted because it isn't your own .npmrc file. ",
        "Only allow dprint to download through it when you trust the .npmrc file because the proxy receives every download ",
        "and the credentials in the setting, including the values of the environment variables it has. ",
        "dprint needs to download `https://example.com/plugin.wasm` from domains that aren't trusted."
      )),
      "{}",
      message
    );
  }

  #[test]
  fn should_trust_the_default_domains_and_their_subdomains() {
    let environment = TestEnvironment::new();
    let trusted_domains = TrustedDomains::from_settings(&environment);
    assert!(trusted_domains.is_trusted(&url("https://plugins.dprint.dev/typescript-0.95.0.wasm")));
    assert!(trusted_domains.is_trusted(&url("https://registry.npmjs.org/@dprint/typescript")));
    assert!(trusted_domains.is_trusted(&url("https://github.com/dprint/dprint-plugin-exec/releases/download/0.5.0/plugin.json")));
    assert!(trusted_domains.is_trusted(&url("https://raw.githubusercontent.com/dprint/dprint/main/dprint.json")));
    assert!(trusted_domains.is_trusted(&url("https://PLUGINS.dprint.dev./plugin.wasm")));
    // not a subdomain
    assert!(!trusted_domains.is_trusted(&url("https://notgithub.com/plugin.wasm")));
    assert!(!trusted_domains.is_trusted(&url("https://github.com.example.com/plugin.wasm")));
    assert!(!trusted_domains.is_trusted(&url("https://example.com/plugin.wasm")));
    // only https
    assert!(!trusted_domains.is_trusted(&url("http://plugins.dprint.dev/plugin.wasm")));
  }

  #[test]
  fn should_add_the_domains_of_the_setting() {
    let environment = TestEnvironment::new();
    let file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    environment
      .write_file(
        &file_path,
        r#"{ "editor": { "trustedDomains": ["npm.example.com", " Other.Example.com. ", "[0:0::1]", "127.1", "npm_registry.corp"] } }"#,
      )
      .unwrap();
    let trusted_domains = TrustedDomains::from_settings(&environment);
    assert!(trusted_domains.is_trusted(&url("https://npm.example.com/@scope/package")));
    assert!(trusted_domains.is_trusted(&url("https://sub.other.example.com/plugin.wasm")));
    // an IP address is compared like the host of a url
    assert!(trusted_domains.is_trusted(&url("https://[::1]/plugin.wasm")));
    assert!(trusted_domains.is_trusted(&url("https://127.0.0.1/plugin.wasm")));
    assert!(trusted_domains.is_trusted(&url("https://npm_registry.corp/@scope/package")));
    // the default ones are still trusted
    assert!(trusted_domains.is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
    assert!(!trusted_domains.is_trusted(&url("https://example.com/plugin.wasm")));
    // the environment doesn't change them
    environment.set_env_var("DPRINT_TRUSTED_DOMAINS", Some("example.com"));
    assert!(!TrustedDomains::from_settings(&environment).is_trusted(&url("https://example.com/plugin.wasm")));
  }

  #[test]
  fn should_trust_no_domains_with_an_unknown_editor_setting() {
    let environment = TestEnvironment::new();
    let file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    // ex. a setting of a newer version of dprint that trusts less
    environment
      .write_file(
        &file_path,
        r#"{ "editor": { "trustedDomains": ["npm.example.com"], "trustDefaultDomains": false } }"#,
      )
      .unwrap();
    // not even the default ones, so every download needs to be approved
    let trusted_domains = TrustedDomains::from_settings(&environment);
    assert_eq!(trusted_domains, TrustedDomains(Vec::new()));
    assert!(!trusted_domains.is_trusted(&url("https://npm.example.com/@scope/package")));
    assert!(!trusted_domains.is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
    // an unknown property outside of "editor" doesn't change them
    environment
      .write_file(&file_path, r#"{ "editor": { "trustedDomains": ["npm.example.com"] }, "other": 1 }"#)
      .unwrap();
    assert!(TrustedDomains::from_settings(&environment).is_trusted(&url("https://npm.example.com/@scope/package")));
  }

  #[test]
  fn should_only_parse_a_host_name_or_an_ip_address_as_a_trusted_domain() {
    assert_eq!(TrustedDomains::parse_domain("npm.example.com").as_deref(), Some("npm.example.com"));
    assert_eq!(TrustedDomains::parse_domain(" .NPM.example-1.com. ").as_deref(), Some("npm.example-1.com"));
    assert_eq!(TrustedDomains::parse_domain("localhost").as_deref(), Some("localhost"));
    assert_eq!(TrustedDomains::parse_domain("127.1").as_deref(), Some("127.0.0.1"));
    assert_eq!(TrustedDomains::parse_domain("[0:0::1]").as_deref(), Some("[::1]"));
    // a url's host may have an underscore
    assert_eq!(TrustedDomains::parse_domain("NPM_registry.corp").as_deref(), Some("npm_registry.corp"));
    // ex. a syntax of a newer version of dprint that trusts less
    for value in [
      "",
      ".",
      "!github.com",
      "github.com/dprint",
      "*.example.com",
      "https://npm.example.com/",
      "npm.example.com:8080",
      "npm..example.com",
      "bücher.example",
      "[::1",
      "[example.com]",
    ] {
      assert_eq!(TrustedDomains::parse_domain(value), None, "{}", value);
    }
  }

  #[test]
  fn should_trust_no_domains_with_a_trusted_domain_this_version_does_not_know() {
    let environment = TestEnvironment::new();
    let file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    // ex. a newer version of dprint that lets the user not trust a default domain, which this one honors
    environment
      .write_file(&file_path, r#"{ "editor": { "trustedDomains": ["!github.com", "npm.example.com"] } }"#)
      .unwrap();
    let trusted_domains = TrustedDomains::from_settings(&environment);
    assert!(!trusted_domains.is_trusted(&url("https://npm.example.com/@scope/package")));
    assert!(!trusted_domains.is_trusted(&url("https://github.com/dprint/plugin.wasm")));
    assert!(!trusted_domains.is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
  }

  #[test]
  fn should_trust_no_domains_when_the_settings_do_not_parse() {
    let environment = TestEnvironment::new();
    let file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    for text in ["{ nope", "[]", r#"{ "editor": { "trustedDomains": "npm.example.com" } }"#] {
      environment.write_file(&file_path, text).unwrap();
      assert_eq!(TrustedDomains::from_settings(&environment), TrustedDomains(Vec::new()), "{}", text);
    }
    // the default ones are trusted without a settings file
    environment.remove_file(&file_path).unwrap();
    assert!(TrustedDomains::from_settings(&environment).is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
  }
}

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::path::Prefix;
use std::sync::Arc;

use parking_lot::Mutex;
use url::Url;

use crate::configuration::ResolveConfigError;
use crate::configuration::read_settings;
use crate::environment::DownloadProxy;
use crate::environment::Environment;
use crate::environment::PathKind;
use crate::resolution::ResolvePluginsError;

/// The domains that the editors download plugins and configuration files from
/// without asking, which the `editor.trustedDomains` setting adds to.
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
  /// It follows the path through the file system (ex. `..` after a symlink),
  /// but never looks up what's on another machine (see `resolve_network_share`).
  ///
  /// IMPORTANT: What's shown and allowed is the path along with where it
  /// resolved to when that's another path (ex. `C:\project\tools\plugin.wasm ->
  /// \\server\share\plugin.wasm` for a symlink), so the user sees the machine
  /// that's contacted and is asked again when the symlink points elsewhere.
  /// Allowing a directory allows what's in it that resolves to what's in where
  /// it resolved to (ex. the files of an npm package in `node_modules`).
  pub fn ensure_allows_path(&self, environment: &impl Environment, path: &Path, base: Option<&Path>) -> Result<(), UntrustedDownloadError> {
    let DownloadPolicy::TrustedDomains(downloads) = self else {
      return Ok(());
    };
    let Some(network_path) = resolve_network_share(environment, path, IS_NET_AUTOMOUNTED) else {
      return Ok(());
    };
    // the file it's relative to was already resolved
    let base_share = base.and_then(|base| get_network_share(base, IS_NET_AUTOMOUNTED));
    if base_share.is_some_and(|base_share| base_share == network_path.share) || downloads.is_path_allowed(path, &network_path.resolved) {
      Ok(())
    } else {
      Err(UntrustedDownloadError {
        download: get_path_download(path, &network_path.resolved),
      })
    }
  }

  /// Gets whether the path may be read without asking the user (see
  /// `ensure_allows_path`). It's for the files that dprint reads on its own
  /// (ex. an `.npmrc` file), which are ignored when they're on another machine.
  pub fn allows_path(&self, environment: &impl Environment, path: &Path, base: Option<&Path>) -> bool {
    self.ensure_allows_path(environment, path, base).is_ok()
  }

  /// Allows downloading what the user was shown and allowed (the downloads
  /// of `UntrustedDownloadError`s) for the rest of the session.
  pub fn allow(&self, downloads: &[String]) {
    if let DownloadPolicy::TrustedDomains(trusted_downloads) = self {
      trusted_downloads.allowed.lock().extend(downloads.iter().cloned());
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
  /// exact ones they were shown. A url ending with a slash allows what's
  /// under it (ex. an npm registry). It also has the proxies of `.npmrc`
  /// files the user allowed downloading through (see `NpmrcProxy::to_download`).
  allowed: Mutex<HashSet<String>>,
}

impl TrustedDownloads {
  fn is_allowed(&self, url_or_path: &str) -> bool {
    self
      .allowed
      .lock()
      .iter()
      .any(|allowed| allowed == url_or_path || (allowed.ends_with('/') && url_or_path.starts_with(allowed.as_str())))
  }

  /// Gets whether reading the path on another machine that resolved to the
  /// other path was allowed, which it also is when a directory it's in was
  /// allowed and the rest of it is the same as the rest of what it resolved to.
  fn is_path_allowed(&self, path: &Path, resolved: &Path) -> bool {
    let (mut path, mut resolved) = (path, resolved);
    loop {
      if self.is_allowed(&get_path_download(path, resolved)) {
        return true;
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

/// The error for something that needs to be downloaded from where the
/// download policy doesn't allow without asking the user.
#[derive(Debug)]
pub struct UntrustedDownloadError {
  /// The url, or the proxy of an `.npmrc` file (see `NpmrcProxy::to_download`).
  pub download: String,
}

impl std::fmt::Display for UntrustedDownloadError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match NpmrcProxy::from_download(&self.download) {
      Some(proxy) => write!(
        f,
        "Downloading through the proxy {} of {} needs to be allowed because the .npmrc file isn't the user's own.",
        proxy.setting, proxy.npmrc_path
      ),
      None => write!(f, "Downloading {} needs to be allowed because its domain isn't trusted.", self.download),
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
      } else {
        err.downcast_ref::<UntrustedDownloadError>()
      }
    })
  }
}

/// The titles of the actions of the message that asks whether to download from
/// domains that aren't trusted.
pub const DOWNLOAD_ACTION_TITLE: &str = "Download";
pub const CANCEL_DOWNLOAD_ACTION_TITLE: &str = "Cancel";

/// Gets the message that asks the user whether to download what a config file
/// needs from domains that aren't trusted, or through the proxies of `.npmrc`
/// files that aren't the user's own.
///
/// Each proxy is said first, starting with its setting and its file, because
/// an editor may cut off a long message (ex. after many long urls) and the
/// user needs to see what's sent to it.
///
/// The settings file isn't said to trust a domain when the editors don't use
/// it (see `read_ignored_settings_message`), which is said instead.
pub fn get_untrusted_download_message(downloads: &[String], ignored_settings_message: Option<&str>) -> String {
  let (proxies, urls): (Vec<_>, Vec<_>) = downloads.iter().partition(|download| NpmrcProxy::from_download(download).is_some());
  let mut messages = Vec::new();
  for proxy in proxies.iter().filter_map(|download| NpmrcProxy::from_download(download)) {
    messages.push(format!(
      concat!(
        "The proxy {} that {} sets isn't trusted because it isn't your own .npmrc file. ",
        "Only allow dprint to download through it when you trust the .npmrc file because the proxy receives every download ",
        "and the credentials in the setting, including the values of the environment variables it has."
      ),
      proxy.setting, proxy.npmrc_path,
    ));
  }
  if !urls.is_empty() {
    messages.push(format!(
      concat!(
        "dprint needs to download {} from domains that aren't trusted. ",
        "Only download them when you trust the configuration file because a download can send information to the domain. {}"
      ),
      urls.iter().map(|url| url.as_str()).collect::<Vec<_>>().join(", "),
      ignored_settings_message.unwrap_or("Add a domain to the editor.trustedDomains setting in dprint's settings file to trust it."),
    ));
  }
  messages.join(" ")
}

/// The domains that are trusted to download from, which includes their
/// subdomains (ex. `githubusercontent.com` includes `raw.githubusercontent.com`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedDomains(Vec<String>);

impl TrustedDomains {
  /// Gets the default trusted domains along with the ones of the
  /// `editor.trustedDomains` setting. It's only a setting and not something
  /// like an environment variable because what the editor is started with
  /// may come from the project (ex. a terminal in the project's directory).
  pub fn from_settings(environment: &impl Environment) -> Self {
    let mut domains = DEFAULT_TRUSTED_DOMAINS.iter().map(|domain| domain.to_string()).collect::<Vec<_>>();
    // IMPORTANT: A property or value in `editor` this version doesn't know may be a
    // setting of a newer version that trusts less, so only the default domains are
    // trusted then, like when the settings can't be read. A warning for either is
    // logged when reading the permissions.
    if let Ok(Some(settings)) = read_settings(environment)
      && settings.get_editor_problem().is_none()
    {
      for domain in settings.editor.trusted_domains.iter().filter_map(|domain| Self::parse_domain(domain)) {
        if !domains.contains(&domain) {
          domains.push(domain);
        }
      }
    }
    TrustedDomains(domains)
  }

  /// Parses a domain of the `editor.trustedDomains` setting as it's compared
  /// to the host of a url, which is `None` when it isn't a host name (letters,
  /// digits, hyphens, and dots between them) or an IP address (an IPv6 one in
  /// brackets). The whitespace and the dots around it are ignored, and so is
  /// the case.
  ///
  /// IMPORTANT: Anything else fails instead of being trusted as something that
  /// matches nothing, because it may be a value of a newer version of dprint
  /// that trusts less (ex. `!github.com` to not trust a default domain).
  pub fn parse_domain(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('.').to_ascii_lowercase();
    let is_ip_v6 = value.starts_with('[');
    let is_host_name = !value.is_empty()
      && value.split('.').all(|label| !label.is_empty())
      && value.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
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
const NPMRC_PROXY_DOWNLOAD_PREFIX: &str = "npmrc-proxy:";

/// Whether a path in `/net` is on another machine, which is where the
/// automounter of macOS mounts the shares of a host when the host is looked up.
const IS_NET_AUTOMOUNTED: bool = cfg!(target_os = "macos");

/// The most symlinks to follow when resolving a path, which is more than the
/// systems follow, so reading a path with more fails anyway.
const MAX_SYMLINKS: usize = 64;

/// Gets what's shown and allowed for reading a path on another machine (see
/// `DownloadPolicy::ensure_allows_path`), which is the path along with what it
/// resolved to when that's another path.
fn get_path_download(path: &Path, resolved: &Path) -> String {
  let path_text = path.display().to_string();
  let resolved_text = resolved.display().to_string();
  if resolved_text == path_text {
    path_text
  } else {
    format!("{} -> {}", path_text, resolved_text)
  }
}

/// Gets where a path that may be on another machine is like
/// `get_network_share`, which follows the path through the file system one
/// component at a time from the root.
///
/// IMPORTANT: The text of the path isn't enough because `..`, the case of
/// `/net` on a case insensitive file system, and symlinks (ex. one in the
/// project that points at a UNC path) may reach another machine. Each
/// component is only looked up once the path before it is known to be local
/// because looking up a host in `/net` connects to it, and a symlink is read
/// without following it, so nothing on another machine is looked up.
fn resolve_network_share(environment: &impl Environment, path: &Path, is_net_automounted: bool) -> Option<NetworkPath> {
  // the components that are left with the next one last, and the path
  // without symlinks or `..` that's resolved so far
  let mut remaining = path.components().rev().filter_map(OwnedComponent::new).collect::<Vec<_>>();
  let mut resolved = PathBuf::new();
  let mut symlink_count = 0;
  while let Some(component) = remaining.pop() {
    match component {
      OwnedComponent::Root(root) => {
        resolved.push(root);
        // ex. a UNC prefix
        if let Some(share) = get_network_share(&resolved, is_net_automounted) {
          return Some(NetworkPath::new(share, resolved, remaining));
        }
      }
      OwnedComponent::Parent => {
        // the parent of the root is the root
        resolved.pop();
      }
      OwnedComponent::Normal(name) => {
        let next = resolved.join(name);
        if let Some(share) = get_network_share(&next, is_net_automounted) {
          return Some(NetworkPath::new(share, next, remaining));
        }
        if environment.path_kind(&next) != Some(PathKind::Symlink) {
          resolved = next;
          continue;
        }
        symlink_count += 1;
        if symlink_count > MAX_SYMLINKS {
          // the whole path, which the file it's relative to never matches
          return Some(NetworkPath {
            share: path.to_string_lossy().to_lowercase(),
            resolved: path.to_path_buf(),
          });
        }
        match environment.read_link(&next) {
          Ok(target) => {
            // a relative target is relative to the symlink's directory, and an absolute one replaces it
            let target = resolved.join(target);
            remaining.extend(target.components().rev().filter_map(OwnedComponent::new));
            resolved = PathBuf::new();
          }
          // reading the path fails the same way
          Err(_) => resolved = next,
        }
      }
    }
  }
  None
}

/// A path on another machine (see `resolve_network_share`).
struct NetworkPath {
  /// Where it is (see `get_network_share`).
  share: String,
  /// The path through the symlinks it reached the share with, which is what's
  /// read (ex. `\\server\share\plugin.wasm` for a symlink to `\\server\share`).
  resolved: PathBuf,
}

impl NetworkPath {
  /// Gets the path on the share at the path that reached it, which is followed
  /// by the components that are left (with the next one last) as they are,
  /// because what's on another machine isn't looked up.
  fn new(share: String, mut resolved: PathBuf, remaining: Vec<OwnedComponent>) -> Self {
    for component in remaining.into_iter().rev() {
      match component {
        OwnedComponent::Root(root) => resolved.push(root),
        OwnedComponent::Parent => resolved.push(".."),
        OwnedComponent::Normal(name) => resolved.push(name),
      }
    }
    NetworkPath { share, resolved }
  }
}

/// A component of a path that's owned (see `resolve_network_share`).
enum OwnedComponent {
  /// A prefix (ex. `C:` or a UNC prefix on Windows) or the root directory.
  Root(OsString),
  Parent,
  Normal(OsString),
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
}

/// Gets where a path that may be on another machine is (ex. the server and
/// share of a UNC path), lowercased because it's case insensitive. It's only
/// from the text of the path, so the path needs to be resolved first (see
/// `resolve_network_share`).
///
/// IMPORTANT: On Windows, only a path on a drive is known to be local. A UNC
/// path isn't, and neither is any other kind of prefix because some of them
/// also reach another machine (ex. `\\.\UNC\server\share` or
/// `\\?\GLOBALROOT\Device\Mup\server\share`). On macOS, a path in `/net` is on
/// another machine because the automounter mounts it.
fn get_network_share(path: &Path, is_net_automounted: bool) -> Option<String> {
  match path.components().next()? {
    Component::Prefix(prefix) => match prefix.kind() {
      Prefix::Disk(_) | Prefix::VerbatimDisk(_) => None,
      Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => Some(format!(
        "{}\\{}",
        server.to_string_lossy().to_lowercase(),
        share.to_string_lossy().to_lowercase()
      )),
      // the share isn't known, so this is the whole path, which the file it's relative to never matches
      _ => Some(path.to_string_lossy().to_lowercase()),
    },
    Component::RootDir if is_net_automounted => {
      let mut components = path.components().skip(1);
      match (components.next(), components.next()) {
        // the file system is case insensitive by default
        (Some(Component::Normal(net)), Some(Component::Normal(host))) if net.eq_ignore_ascii_case("net") => {
          Some(format!("/net/{}", host.to_string_lossy().to_lowercase()))
        }
        _ => None,
      }
    }
    _ => None,
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;

  fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
  }

  #[test]
  fn should_only_allow_exactly_what_the_user_allowed() {
    let environment = TestEnvironment::new();
    let policy = DownloadPolicy::for_editor(&environment);
    assert!(policy.ensure_allows(&url("https://plugins.dprint.dev/plugin.wasm")).is_ok());
    let err = policy.ensure_allows(&url("https://example.com/plugin.wasm")).unwrap_err();
    assert_eq!(err.download, "https://example.com/plugin.wasm");
    policy.allow(&["https://example.com/plugin.wasm".to_string(), "https://npm.example.com/".to_string()]);
    assert!(policy.allows(&url("https://example.com/plugin.wasm")));
    // not what redirects to
    assert!(!policy.allows(&url("https://example.com/other.wasm")));
    // a url ending with a slash allows what's under it (ex. an npm registry)
    assert!(policy.allows(&url("https://npm.example.com/@scope/package")));
    assert!(!policy.allows(&url("https://npm.example.com.evil.com/@scope/package")));
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
    assert!(
      policy
        .ensure_allows_path(&environment, Path::new("/project/link/../plugin.wasm"), None)
        .is_err()
    );
  }

  #[test]
  fn should_resolve_the_path_to_net_before_reading_it() {
    let environment = TestEnvironment::new();
    environment.mk_dir_all("/home/me/project").unwrap();
    environment.mk_dir_all("/a/b").unwrap();
    let share = |path: &str| resolve_network_share(&environment, Path::new(path), true).map(|network_path| network_path.share);
    let resolved = |path: &str| resolve_network_share(&environment, Path::new(path), true).map(|network_path| network_path.resolved);
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
    assert_eq!(
      resolved("/home/me/project/net_link/host/a/../plugin.wasm").as_deref(),
      Some(Path::new("/net/host/a/../plugin.wasm"))
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
    assert!(resolve_network_share(&environment, Path::new("/net/host/plugin.wasm"), false).is_none());
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
      "Downloading through the proxy https://${TOKEN}@proxy.example of /project/.npmrc needs to be allowed because the .npmrc file isn't the user's own."
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
  fn should_say_what_needs_to_be_allowed() {
    let proxy = NpmrcProxy {
      npmrc_path: "/project/.npmrc".to_string(),
      setting: "https://${TOKEN}@proxy.example".to_string(),
    };
    let url = "https://example.com/plugin.wasm".to_string();
    let message = get_untrusted_download_message(std::slice::from_ref(&url), None);
    assert!(
      message.starts_with("dprint needs to download https://example.com/plugin.wasm from domains that aren't trusted."),
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
        "The proxy https://${TOKEN}@proxy.example that /project/.npmrc sets isn't trusted because it isn't your own .npmrc file. ",
        "Only allow dprint to download through it when you trust the .npmrc file because the proxy receives every download ",
        "and the credentials in the setting, including the values of the environment variables it has. ",
        "dprint needs to download https://example.com/plugin.wasm from domains that aren't trusted."
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
        r#"{ "editor": { "trustedDomains": ["npm.example.com", " Other.Example.com. ", "[0:0::1]", "127.1"] } }"#,
      )
      .unwrap();
    let trusted_domains = TrustedDomains::from_settings(&environment);
    assert!(trusted_domains.is_trusted(&url("https://npm.example.com/@scope/package")));
    assert!(trusted_domains.is_trusted(&url("https://sub.other.example.com/plugin.wasm")));
    // an IP address is compared like the host of a url
    assert!(trusted_domains.is_trusted(&url("https://[::1]/plugin.wasm")));
    assert!(trusted_domains.is_trusted(&url("https://127.0.0.1/plugin.wasm")));
    // the default ones are still trusted
    assert!(trusted_domains.is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
    assert!(!trusted_domains.is_trusted(&url("https://example.com/plugin.wasm")));
    // the environment doesn't change them
    environment.set_env_var("DPRINT_TRUSTED_DOMAINS", Some("example.com"));
    assert!(!TrustedDomains::from_settings(&environment).is_trusted(&url("https://example.com/plugin.wasm")));
  }

  #[test]
  fn should_only_trust_the_default_domains_with_an_unknown_editor_setting() {
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
    let trusted_domains = TrustedDomains::from_settings(&environment);
    assert!(!trusted_domains.is_trusted(&url("https://npm.example.com/@scope/package")));
    assert!(trusted_domains.is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
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
      "npm_example.com",
      "bücher.example",
      "[::1",
      "[example.com]",
    ] {
      assert_eq!(TrustedDomains::parse_domain(value), None, "{}", value);
    }
  }

  #[test]
  fn should_only_trust_the_default_domains_with_a_trusted_domain_this_version_does_not_know() {
    let environment = TestEnvironment::new();
    let file_path = crate::configuration::get_settings_file_path(&environment).unwrap();
    environment.mk_dir_all(file_path.parent().unwrap()).unwrap();
    // ex. a newer version of dprint that lets the user not trust a default domain
    environment
      .write_file(&file_path, r#"{ "editor": { "trustedDomains": ["!github.com", "npm.example.com"] } }"#)
      .unwrap();
    let trusted_domains = TrustedDomains::from_settings(&environment);
    assert!(!trusted_domains.is_trusted(&url("https://npm.example.com/@scope/package")));
    assert!(trusted_domains.is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
  }
}

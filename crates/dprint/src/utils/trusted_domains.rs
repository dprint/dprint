use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;
use url::Url;

use crate::configuration::ResolveConfigError;
use crate::configuration::read_settings;
use crate::environment::DownloadProxy;
use crate::environment::Environment;
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
/// editors only download from the trusted domains without asking. A file on a
/// network share or a mounted drive is read like a local file because the user
/// chose to make it reachable like one.
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
  /// The urls the user allowed downloading this session, which are only the
  /// exact ones they were shown. A url ending with a slash allows what's
  /// under it (ex. an npm registry). It also has the proxies of `.npmrc`
  /// files the user allowed downloading through (see `NpmrcProxy::to_download`).
  allowed: Mutex<HashSet<String>>,
}

impl TrustedDownloads {
  fn is_allowed(&self, url: &str) -> bool {
    self
      .allowed
      .lock()
      .iter()
      .any(|allowed| allowed == url || (allowed.ends_with('/') && url.starts_with(allowed.as_str())))
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
pub fn get_untrusted_download_message(downloads: &[String]) -> String {
  let (proxies, urls): (Vec<_>, Vec<_>) = downloads.iter().partition(|download| NpmrcProxy::from_download(download).is_some());
  let mut messages = Vec::new();
  if !urls.is_empty() {
    messages.push(format!(
      concat!(
        "dprint needs to download {} from domains that aren't trusted. ",
        "Only download them when you trust the configuration file because a download can send information to the domain. ",
        "Add a domain to the editor.trustedDomains setting in dprint's settings file to trust it."
      ),
      urls.iter().map(|url| url.as_str()).collect::<Vec<_>>().join(", "),
    ));
  }
  for proxy in proxies.iter().filter_map(|download| NpmrcProxy::from_download(download)) {
    messages.push(format!(
      concat!(
        "dprint needs to download through the proxy {} that {} sets, which isn't trusted because it isn't your own .npmrc file. ",
        "Only allow it when you trust the .npmrc file because the proxy receives every download and the credentials in the setting, ",
        "including the values of the environment variables it has."
      ),
      proxy.setting, proxy.npmrc_path,
    ));
  }
  messages.join(" ")
}

/// Gets how to show a download of an `UntrustedDownloadError` (ex. in a
/// list of what the user decided), which is the url or the proxy setting of
/// an `.npmrc` file with its path.
pub fn get_download_display(download: &str) -> String {
  match NpmrcProxy::from_download(download) {
    Some(proxy) => format!("{} ({})", proxy.setting, proxy.npmrc_path),
    None => download.to_string(),
  }
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
    // a warning that the settings can't be read is logged when reading the permissions
    if let Ok(Some(settings)) = read_settings(environment) {
      for domain in settings.editor.trusted_domains {
        let domain = domain.trim().trim_matches('.').to_ascii_lowercase();
        if !domain.is_empty() && !domains.contains(&domain) {
          domains.push(domain);
        }
      }
    }
    TrustedDomains(domains)
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
    assert_eq!(get_download_display(&url), url);
    assert_eq!(get_download_display(&proxy.to_download()), "https://${TOKEN}@proxy.example (/project/.npmrc)");
    let message = get_untrusted_download_message(&[proxy.to_download(), url]);
    assert!(
      message.starts_with("dprint needs to download https://example.com/plugin.wasm from domains that aren't trusted."),
      "{}",
      message
    );
    assert!(
      message.contains("dprint needs to download through the proxy https://${TOKEN}@proxy.example that /project/.npmrc sets,"),
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
        r#"{ "editor": { "trustedDomains": ["npm.example.com", " Other.Example.com. ", ""] } }"#,
      )
      .unwrap();
    let trusted_domains = TrustedDomains::from_settings(&environment);
    assert!(trusted_domains.is_trusted(&url("https://npm.example.com/@scope/package")));
    assert!(trusted_domains.is_trusted(&url("https://sub.other.example.com/plugin.wasm")));
    // the default ones are still trusted
    assert!(trusted_domains.is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
    assert!(!trusted_domains.is_trusted(&url("https://example.com/plugin.wasm")));
    // the environment doesn't change them
    environment.set_env_var("DPRINT_TRUSTED_DOMAINS", Some("example.com"));
    assert!(!TrustedDomains::from_settings(&environment).is_trusted(&url("https://example.com/plugin.wasm")));
  }
}

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::Mutex;
use url::Url;

use crate::configuration::ResolveConfigError;
use crate::configuration::read_settings;
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

  /// Errors when the url may not be downloaded without asking the user.
  pub fn ensure_allows(&self, url: &Url) -> Result<(), UntrustedDownloadError> {
    if self.allows(url) {
      Ok(())
    } else {
      Err(UntrustedDownloadError { url: url.to_string() })
    }
  }

  /// Allows downloading what the user was shown and allowed (the urls of
  /// `UntrustedDownloadError`s) for the rest of the session.
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
  /// under it (ex. an npm registry).
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

/// The error for something that needs to be downloaded from where the
/// download policy doesn't allow without asking the user.
#[derive(Debug, thiserror::Error)]
#[error("Downloading {url} needs to be allowed because its domain isn't trusted.")]
pub struct UntrustedDownloadError {
  pub url: String,
}

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
/// needs from domains that aren't trusted.
pub fn get_untrusted_download_message(downloads: &[String]) -> String {
  format!(
    concat!(
      "dprint needs to download {} from domains that aren't trusted. ",
      "Only download them when you trust the configuration file because a download can send information to the domain. ",
      "Add a domain to the editor.trustedDomains setting in dprint's settings file to trust it."
    ),
    downloads.join(", "),
  )
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
    assert_eq!(err.url, "https://example.com/plugin.wasm");
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

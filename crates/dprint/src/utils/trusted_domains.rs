use std::collections::HashSet;
use std::path::Component;
use std::path::Path;
use std::path::Prefix;
use std::sync::Arc;

use parking_lot::Mutex;
use url::Url;

use crate::configuration::ResolveConfigError;
use crate::environment::Environment;
use crate::resolution::ResolvePluginsError;

/// The domains that the editors download plugins and configuration files from
/// without asking, which `DPRINT_TRUSTED_DOMAINS` replaces.
pub const DEFAULT_TRUSTED_DOMAINS: [&str; 4] = ["registry.npmjs.org", "plugins.dprint.dev", "github.com", "githubusercontent.com"];
/// The environment variable with the comma separated trusted domains, which
/// replace the default ones.
pub const TRUSTED_DOMAINS_ENV_VAR_NAME: &str = "DPRINT_TRUSTED_DOMAINS";

/// What may be downloaded without asking the user, which the environment
/// enforces for every request it sends (see `Environment::set_download_policy`).
///
/// IMPORTANT: Opening a project in an editor isn't the same as choosing to
/// download what its configuration file says. Downloading from any domain would
/// let a configuration file send information to a server of its choosing (ex.
/// that the project was opened, from where, and anything in the url), so the
/// editors only download from the trusted domains without asking. The same
/// goes for reading a file on another machine (ex. a UNC path on Windows).
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
      trusted_domains: TrustedDomains::from_environment(environment),
      allowed: Default::default(),
    }))
  }

  /// Gets whether the url may be downloaded without asking the user.
  pub fn allows(&self, url: &Url) -> bool {
    match self {
      DownloadPolicy::All => true,
      DownloadPolicy::TrustedDomains(downloads) => downloads.trusted_domains.is_trusted(url) || downloads.is_allowed(url.as_str()),
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

  /// Errors when the path is on another machine and may not be read without
  /// asking the user, which is allowed when the file it's relative to is on the
  /// same network share (ex. a config file in a project on a share).
  pub fn ensure_allows_path(&self, path: &Path, base: Option<&Path>) -> Result<(), UntrustedDownloadError> {
    let DownloadPolicy::TrustedDomains(downloads) = self else {
      return Ok(());
    };
    let Some(share) = get_network_share(path) else {
      return Ok(());
    };
    let path_text = path.display().to_string();
    if base.and_then(get_network_share).is_some_and(|base_share| base_share == share) || downloads.is_allowed(&path_text) {
      Ok(())
    } else {
      Err(UntrustedDownloadError { url: path_text })
    }
  }

  /// Allows downloading what the user was shown and allowed (the urls and
  /// paths of `UntrustedDownloadError`s) for the rest of the session.
  pub fn allow(&self, downloads: &[String]) {
    if let DownloadPolicy::TrustedDomains(trusted_downloads) = self {
      trusted_downloads.allowed.lock().extend(downloads.iter().cloned());
    }
  }
}

/// What may be downloaded with the `TrustedDomains` download policy.
#[derive(Debug)]
pub struct TrustedDownloads {
  trusted_domains: TrustedDomains,
  /// The urls and paths the user allowed downloading this session, which are
  /// only the exact ones they were shown. A url ending with a slash allows
  /// what's under it (ex. an npm registry).
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
      "Set the {} environment variable to change the trusted domains."
    ),
    downloads.join(", "),
    TRUSTED_DOMAINS_ENV_VAR_NAME,
  )
}

/// The domains that are trusted to download from, which includes their
/// subdomains (ex. `githubusercontent.com` includes `raw.githubusercontent.com`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedDomains(Vec<String>);

impl TrustedDomains {
  /// Gets the trusted domains, which `DPRINT_TRUSTED_DOMAINS` replaces.
  pub fn from_environment(environment: &impl Environment) -> Self {
    match environment.env_var(TRUSTED_DOMAINS_ENV_VAR_NAME) {
      Some(value) => Self::parse(&value.to_string_lossy()),
      None => TrustedDomains(DEFAULT_TRUSTED_DOMAINS.iter().map(|domain| domain.to_string()).collect()),
    }
  }

  fn parse(text: &str) -> Self {
    TrustedDomains(
      text
        .split(',')
        .map(|domain| domain.trim().trim_matches('.').to_ascii_lowercase())
        .filter(|domain| !domain.is_empty())
        .collect(),
    )
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

/// Gets the server and share of a path on another machine (ex. a UNC path on
/// Windows), lowercased because they're case insensitive.
fn get_network_share(path: &Path) -> Option<(String, String)> {
  match path.components().next()? {
    Component::Prefix(prefix) => match prefix.kind() {
      Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
        Some((server.to_string_lossy().to_lowercase(), share.to_string_lossy().to_lowercase()))
      }
      _ => None,
    },
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

  #[cfg(windows)]
  #[test]
  fn should_not_allow_reading_from_another_machine() {
    let environment = TestEnvironment::new();
    let policy = DownloadPolicy::for_editor(&environment);
    assert!(
      policy
        .ensure_allows_path(Path::new(r"C:\project\plugin.wasm"), Some(Path::new(r"C:\project")))
        .is_ok()
    );
    let err = policy
      .ensure_allows_path(Path::new(r"\\server\share\plugin.wasm"), Some(Path::new(r"C:\project")))
      .unwrap_err();
    assert_eq!(err.url, r"\\server\share\plugin.wasm");
    assert!(policy.ensure_allows_path(Path::new(r"\\?\UNC\server\share\plugin.wasm"), None).is_err());
    // a config file on the share may use what's on it
    assert!(
      policy
        .ensure_allows_path(Path::new(r"\\SERVER\share\plugin.wasm"), Some(Path::new(r"\\server\Share\project")))
        .is_ok()
    );
    assert!(
      policy
        .ensure_allows_path(Path::new(r"\\server\other\plugin.wasm"), Some(Path::new(r"\\server\share\project")))
        .is_err()
    );
    policy.allow(&[r"\\server\share\plugin.wasm".to_string()]);
    assert!(policy.ensure_allows_path(Path::new(r"\\server\share\plugin.wasm"), None).is_ok());
    // the cli reads anything
    assert!(DownloadPolicy::All.ensure_allows_path(Path::new(r"\\server\share\plugin.wasm"), None).is_ok());
  }

  #[test]
  fn should_trust_the_default_domains_and_their_subdomains() {
    let environment = TestEnvironment::new();
    let trusted_domains = TrustedDomains::from_environment(&environment);
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
  fn should_replace_the_default_domains_with_the_environment_variable() {
    let environment = TestEnvironment::new();
    environment.set_env_var(TRUSTED_DOMAINS_ENV_VAR_NAME, Some(" example.com , .Other.dev, "));
    let trusted_domains = TrustedDomains::from_environment(&environment);
    assert!(trusted_domains.is_trusted(&url("https://example.com/plugin.wasm")));
    assert!(trusted_domains.is_trusted(&url("https://sub.other.dev/plugin.wasm")));
    assert!(!trusted_domains.is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
    // nothing is trusted when it's empty
    environment.set_env_var(TRUSTED_DOMAINS_ENV_VAR_NAME, Some(""));
    assert!(!TrustedDomains::from_environment(&environment).is_trusted(&url("https://plugins.dprint.dev/plugin.wasm")));
  }
}

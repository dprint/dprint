use std::rc::Rc;

use url::Url;

use crate::configuration::ResolveConfigError;
use crate::environment::Environment;

/// The domains that the editors download plugins and configuration files from
/// without asking, which `DPRINT_TRUSTED_DOMAINS` replaces.
pub const DEFAULT_TRUSTED_DOMAINS: [&str; 4] = ["registry.npmjs.org", "plugins.dprint.dev", "github.com", "githubusercontent.com"];
/// The environment variable with the comma separated trusted domains, which
/// replace the default ones.
pub const TRUSTED_DOMAINS_ENV_VAR_NAME: &str = "DPRINT_TRUSTED_DOMAINS";

/// What may be downloaded without asking the user.
///
/// IMPORTANT: Opening a project in an editor isn't the same as choosing to
/// download what its configuration file says. Downloading from any domain would
/// let a configuration file send information to a server of its choosing (ex.
/// that the project was opened, from where, and anything in the url), so the
/// editors only download from the trusted domains without asking.
#[derive(Debug, Clone)]
pub enum DownloadPolicy {
  /// Anything may be downloaded (ex. for the CLI, which the user ran).
  All,
  /// Only what's on the trusted domains may be downloaded.
  TrustedDomains(Rc<TrustedDomains>),
}

impl DownloadPolicy {
  /// Gets the policy of the editors, which only download from the trusted domains.
  pub fn for_editor(environment: &impl Environment) -> Self {
    DownloadPolicy::TrustedDomains(Rc::new(TrustedDomains::from_environment(environment)))
  }

  /// Gets whether the url may be downloaded without asking the user.
  pub fn allows(&self, url: &Url) -> bool {
    match self {
      DownloadPolicy::All => true,
      DownloadPolicy::TrustedDomains(trusted_domains) => trusted_domains.is_trusted(url),
    }
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
      match err.downcast_ref::<ResolveConfigError>() {
        Some(ResolveConfigError::Other(inner)) => Self::find(inner),
        _ => err.downcast_ref::<UntrustedDownloadError>(),
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

#[cfg(test)]
mod test {
  use super::*;
  use crate::environment::TestEnvironment;

  fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
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

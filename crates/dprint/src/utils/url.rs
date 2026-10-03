use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use anyhow::bail;
use deno_terminal::colors;
use parking_lot::Mutex;
use url::Url;

use super::Logger;
use super::certs::get_root_certs;
use super::logging::ProgressBarStyle;
use super::logging::ProgressBars;
use super::no_proxy::NoProxy;
use crate::environment::DownloadOptions;
use crate::environment::DownloadProxy;
use crate::environment::DownloadedFile;

const MAX_RETRIES: u8 = 2;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait on a server that was connected to before it starts responding.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum AgentKind {
  Http,
  Https,
}

trait ProxyProvider {
  fn get_proxy(&self, kind: AgentKind) -> Option<&'static str>;
}

struct RealProxyUrlProvider;

impl ProxyProvider for RealProxyUrlProvider {
  fn get_proxy(&self, kind: AgentKind) -> Option<&'static str> {
    fn read_proxy_env_var(env_var_name: &str) -> Option<String> {
      // too much of a hassle to create a seam for the env var reading
      // and this struct is created before an env is created anyway
      #[allow(clippy::disallowed_methods)]
      std::env::var(env_var_name.to_uppercase())
        .ok()
        .or_else(|| std::env::var(env_var_name.to_lowercase()).ok())
        .filter(|v| !v.is_empty())
    }

    static HTTP_PROXY: OnceLock<Option<String>> = OnceLock::new();
    static HTTPS_PROXY: OnceLock<Option<String>> = OnceLock::new();

    match kind {
      AgentKind::Http => HTTP_PROXY.get_or_init(|| read_proxy_env_var("HTTP_PROXY")).as_deref(),
      AgentKind::Https => HTTPS_PROXY.get_or_init(|| read_proxy_env_var("HTTPS_PROXY")).as_deref(),
    }
  }
}

struct AgentWithProxy {
  agent: ureq::Agent,
  /// The proxy the agent sends its requests through.
  proxy: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AgentKey {
  kind: AgentKind,
  proxy: Option<String>,
  ignore_certificates: bool,
}

struct AgentStore<TProxyUrlProvider: ProxyProvider> {
  agents: Mutex<HashMap<AgentKey, ureq::Agent>>,
  logger: Arc<Logger>,
  no_proxy: NoProxy,
  proxy_url_provider: TProxyUrlProvider,
  unsafely_ignore_certificates: Option<UnsafelyIgnoreCertificates>,
}

impl<TProxyUrlProvider: ProxyProvider> AgentStore<TProxyUrlProvider> {
  /// Gets the agent for requesting the url.
  pub fn get(&self, kind: AgentKind, url: &Url, proxy: DownloadProxy<'_>) -> Result<AgentWithProxy> {
    let proxy = match proxy {
      DownloadProxy::Environment => self.proxy_url_provider.get_proxy(kind),
      DownloadProxy::Url(proxy) => Some(proxy),
      DownloadProxy::Direct => None,
    };
    let proxy = proxy.filter(|_| match url.host_str() {
      Some(host) => !self.no_proxy.contains(host),
      None => true,
    });
    let key = AgentKey {
      kind,
      proxy: proxy.map(|proxy| proxy.to_string()),
      ignore_certificates: kind == AgentKind::Https
        && self
          .unsafely_ignore_certificates
          .as_ref()
          .is_some_and(|ignored| url.host_str().is_some_and(|host| ignored.ignores_host(host))),
    };
    let mut agents = self.agents.lock();
    let agent = match agents.get(&key) {
      Some(agent) => agent.clone(),
      None => {
        // blocking the lock isn't too bad here because generally
        // there will only ever be one of these created ever
        let agent = self.build_agent(&key)?;
        agents.insert(key.clone(), agent.clone());
        agent
      }
    };
    Ok(AgentWithProxy { agent, proxy: key.proxy })
  }

  fn build_agent(&self, key: &AgentKey) -> Result<ureq::Agent> {
    static INSTALLED_PROVIDER: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let proxy = match &key.proxy {
      Some(proxy) => Some(parse_proxy(proxy)?),
      None => None,
    };
    // the connection to an https proxy is made over TLS as well
    let uses_tls = key.kind == AgentKind::Https || proxy.as_ref().is_some_and(|proxy| proxy.protocol() == ureq::ProxyProtocol::Https);
    let mut config = ureq::Agent::config_builder()
      // statuses and redirects are handled by the downloader
      .http_status_as_error(false)
      .max_redirects(0)
      .max_redirects_will_error(false)
      .timeout_connect(Some(CONNECT_TIMEOUT))
      .timeout_recv_response(Some(RESPONSE_TIMEOUT))
      // this needs to be set even when there's no proxy because ureq
      // defaults to reading the proxy from the environment itself
      .proxy(proxy);
    if uses_tls {
      INSTALLED_PROVIDER.get_or_init(|| {
        if let Some(ignored) = &self.unsafely_ignore_certificates {
          log_warn!(
            self.logger,
            "{} Unsafely ignoring {} TLS certificates!",
            colors::yellow("Warning"),
            if ignored.0.is_empty() { "all" } else { "some" }
          );
        }
        let previous_provider = rustls::crypto::ring::default_provider().install_default();
        debug_assert!(previous_provider.is_ok());
      });

      #[allow(clippy::disallowed_methods)]
      let root_certs = get_root_certs(&self.logger, &|env_var| std::env::var(env_var).ok(), &|file_path| std::fs::read(file_path))?;
      let root_certs = root_certs
        .iter()
        .map(|cert| ureq::tls::Certificate::from_der(cert.as_ref()).to_owned())
        .collect::<Vec<_>>();
      config = config.tls_config(
        ureq::tls::TlsConfig::builder()
          .root_certs(ureq::tls::RootCerts::Specific(Arc::new(root_certs)))
          // note: this also applies to the connection made to an https proxy
          // because ureq uses the one config for both
          .disable_verification(key.ignore_certificates)
          .build(),
      );
    }
    Ok(ureq::Agent::new_with_config(config.build()))
  }
}

#[derive(Debug, Clone)]
pub struct UnsafelyIgnoreCertificates(Arc<Vec<String>>);

impl UnsafelyIgnoreCertificates {
  pub fn new(ic_allowlist: Vec<String>) -> Self {
    Self(Arc::new(ic_allowlist))
  }

  pub fn from_env() -> Option<Self> {
    let var = std::env::var_os("DPRINT_IGNORE_CERTS")?;
    if var == "1" {
      Some(Self::new(Vec::new()))
    } else {
      let var = var.to_str()?;
      Some(Self::new(var.split(",").map(|v| v.to_string()).collect()))
    }
  }

  /// Whether the certificates of the host should not be verified, which is
  /// every host when no specific ones were provided.
  fn ignores_host(&self, host: &str) -> bool {
    // an IPv6 address is in brackets in a url
    let host = host.trim_start_matches('[').trim_end_matches(']');
    self.0.is_empty() || self.0.iter().any(|ignored| ignored == host)
  }
}

pub struct RealUrlDownloader {
  progress_bars: Option<Arc<ProgressBars>>,
  agent_store: AgentStore<RealProxyUrlProvider>,
  logger: Arc<Logger>,
}

impl RealUrlDownloader {
  pub fn new(
    progress_bars: Option<Arc<ProgressBars>>,
    logger: Arc<Logger>,
    no_proxy: NoProxy,
    unsafely_ignore_certificates: Option<UnsafelyIgnoreCertificates>,
  ) -> Result<Self> {
    Ok(Self {
      progress_bars,
      agent_store: AgentStore {
        agents: Default::default(),
        logger: logger.clone(),
        no_proxy,
        proxy_url_provider: RealProxyUrlProvider,
        unsafely_ignore_certificates,
      },
      logger,
    })
  }

  pub fn download(&self, url: &Url, options: DownloadOptions<'_>) -> Result<Option<DownloadedFile>> {
    let agent = self.get_agent(url, options.proxy)?;
    self.download_with_retries(url, options.auth, &agent)
  }

  fn download_with_retries(&self, url: &Url, auth: Option<&str>, agent: &AgentWithProxy) -> Result<Option<DownloadedFile>> {
    let mut last_error = None;
    for retry_count in 0..(MAX_RETRIES + 1) {
      match self.inner_download(url, auth, retry_count, agent) {
        Ok(result) => return Ok(result),
        Err(err) => {
          if retry_count < MAX_RETRIES {
            log_debug!(self.logger, "Error downloading {} ({}/{}): {:#}", url, retry_count, MAX_RETRIES, err);
          }
          last_error = Some(err);
        }
      }
    }
    Err(last_error.unwrap())
  }

  #[cfg(test)]
  pub fn download_no_retries_for_testing(&self, url: &str) -> Result<Option<Vec<u8>>> {
    let url = Url::parse(url)?;
    let agent = self.get_agent(&url, DownloadProxy::Environment)?;
    Ok(self.inner_download(&url, None, 0, &agent)?.map(|r| r.content))
  }

  fn get_agent(&self, url: &Url, proxy: DownloadProxy<'_>) -> Result<AgentWithProxy> {
    let kind = match url.scheme() {
      "https" => AgentKind::Https,
      "http" => AgentKind::Http,
      _ => bail!("Not implemented url scheme: {}", url),
    };
    // this is expensive, but we're already in a blocking task here
    self.agent_store.get(kind, url, proxy)
  }

  fn inner_download(&self, url: &Url, auth: Option<&str>, retry_count: u8, agent: &AgentWithProxy) -> Result<Option<DownloadedFile>> {
    let mut request = agent.agent.get(url.as_str());
    if let Some(auth) = auth {
      request = request.header("Authorization", auth);
    }
    let resp = match request.call() {
      Ok(resp) => resp,
      Err(err) => {
        bail!("Error downloading {} - {}", url, get_request_error_message(url, &err, agent.proxy.as_deref()))
      }
    };

    let status = resp.status();
    if status == ureq::http::StatusCode::NOT_FOUND {
      return Ok(None);
    }
    let headers: HashMap<String, String> = resp
      .headers()
      .iter()
      .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.to_string())))
      .collect();

    if status.is_redirection() {
      if !headers.contains_key("location") {
        bail!("Error downloading {} - {} without a location to redirect to", url, status.as_u16());
      }
      return Ok(Some(DownloadedFile { headers, content: vec![] }));
    }
    if !status.is_success() {
      match status.canonical_reason() {
        Some(reason) => bail!("Error downloading {} - {} {}", url, status.as_u16(), reason),
        None => bail!("Error downloading {} - {}", url, status.as_u16()),
      }
    }

    let total_size = headers.get("content-length").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let mut reader = resp.into_body().into_reader();
    match read_response(url, retry_count, &mut reader, total_size, self.progress_bars.as_deref()) {
      Ok(content) => Ok(Some(DownloadedFile { headers, content })),
      Err(err) => bail!("Error downloading {} - {:#}", url, err),
    }
  }
}

fn read_response(url: &Url, retry_count: u8, reader: &mut impl Read, total_size: usize, progress_bars: Option<&ProgressBars>) -> Result<Vec<u8>> {
  let mut final_bytes = Vec::new();
  final_bytes.try_reserve_exact(total_size)?;
  if let Some(progress_bars) = &progress_bars {
    let mut buf: [u8; 512] = [0; 512]; // ensure progress bars update often
    let mut message = format!("Downloading {}", url);
    if retry_count > 0 {
      message.push_str(&format!(" (Retry {}/{})", retry_count, MAX_RETRIES))
    }
    let pb = progress_bars.add_progress(message, ProgressBarStyle::Download, total_size);
    loop {
      let bytes_read = reader.read(&mut buf)?;
      if bytes_read == 0 {
        break;
      }
      final_bytes.extend(&buf[..bytes_read]);
      pb.set_position(final_bytes.len());
    }
    pb.finish();
  } else {
    reader.read_to_end(&mut final_bytes)?;
  }
  Ok(final_bytes)
}

/// Describes why a request failed without repeating the url, which ureq's
/// own message isn't written for showing to a user.
fn get_request_error_message(url: &Url, err: &ureq::Error, proxy: Option<&str>) -> String {
  let host = url.host_str().unwrap_or(url.as_str());
  let proxy = proxy.map(display_proxy);
  let target = match &proxy {
    Some(proxy) => format!("{} through the proxy {}", host, proxy),
    None => host.to_string(),
  };
  match err {
    ureq::Error::Timeout(ureq::Timeout::Connect) => format!("Timed out connecting to {}.", target),
    ureq::Error::Timeout(ureq::Timeout::RecvResponse) => format!("Timed out waiting for a response from {}.", target),
    ureq::Error::Timeout(reason) => format!("Timed out requesting {} ({}).", target, reason),
    ureq::Error::ConnectionFailed => format!("Could not connect to {}.", target),
    // ureq only provides the status code as text
    ureq::Error::ConnectProxyFailed(reason) if reason.contains("407") => {
      format!(
        "Could not connect to {}: the proxy requires authentication or rejected the credentials.",
        target
      )
    }
    ureq::Error::ConnectProxyFailed(reason) => format!("Could not connect to {}: {}.", target, reason),
    ureq::Error::Io(err) if is_connect_error(err) => format!("Could not connect to {}: {}.", target, err),
    // these don't say what failed (ex. a host that couldn't be resolved may
    // be the proxy's), so at least say a proxy was involved
    err => match &proxy {
      Some(proxy) => format!("{} (using the proxy {})", get_error_text(err), proxy),
      None => get_error_text(err),
    },
  }
}

fn get_error_text(err: &ureq::Error) -> String {
  match err {
    // without ureq's "io: " prefix
    ureq::Error::Io(err) => err.to_string(),
    err => err.to_string(),
  }
}

fn is_connect_error(err: &std::io::Error) -> bool {
  matches!(
    err.kind(),
    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::HostUnreachable | std::io::ErrorKind::NetworkUnreachable | std::io::ErrorKind::AddrNotAvailable
  )
}

/// Creates a proxy from the text of a proxy setting.
///
/// ureq parses the text as a uri, which is stricter than what's been accepted
/// in these settings in the past, so the text is adjusted to keep the settings
/// people have working where possible.
fn parse_proxy(text: &str) -> Result<ureq::Proxy> {
  // a trailing slash is not allowed when there's no scheme
  let trimmed_text = text.trim_end_matches('/');
  let (scheme, rest) = match trimmed_text.split_once("://") {
    Some((scheme, rest)) => (Some(scheme.to_ascii_lowercase()), rest),
    None => (None, trimmed_text),
  };
  let normalized_text = match scheme.as_deref() {
    // have the proxy resolve the host instead of resolving it locally, which
    // is what a socks5 proxy has always been used for here
    Some("socks" | "socks5") => format!("socks5h://{}", rest),
    Some(scheme) => format!("{}://{}", scheme, rest),
    None => rest.to_string(),
  };
  let has_credentials = rest.contains('@');
  let is_known_scheme = scheme.as_deref().is_none_or(|scheme| ureq::ProxyProtocol::try_from(scheme).is_ok());
  let credentials_hint = if has_credentials && is_known_scheme {
    " Its username or password may contain a character that is not supported."
  } else {
    ""
  };
  let proxy = match ureq::Proxy::new(&normalized_text) {
    Ok(proxy) => proxy,
    Err(_) => bail!("Invalid proxy {}.{}", display_proxy(text), credentials_hint),
  };
  // ureq takes a `/`, `?` or `#` in the credentials as the end of the
  // host, which leaves it connecting somewhere else without credentials
  if has_credentials && proxy.username().is_none() {
    bail!("Invalid proxy {}.{}", display_proxy(text), credentials_hint);
  }
  Ok(proxy)
}

/// The proxy without its credentials, for showing in messages.
fn display_proxy(proxy: &str) -> String {
  let (scheme, rest) = match proxy.split_once("://") {
    Some((scheme, rest)) => (Some(scheme), rest),
    None => (None, proxy),
  };
  let address = rest.rsplit_once('@').map(|(_, address)| address).unwrap_or(rest).trim_end_matches('/');
  match scheme {
    Some(scheme) => format!("{}://{}", scheme, address),
    None => address.to_string(),
  }
}

#[cfg(test)]
mod test {
  use std::io::ErrorKind;
  use std::process::Child;
  use std::process::Command;
  use std::process::Stdio;
  use std::sync::Arc;
  use std::time::Duration;

  use crate::environment::DownloadOptions;
  use crate::environment::DownloadProxy;
  use crate::utils::LogLevel;
  use crate::utils::Logger;
  use crate::utils::LoggerOptions;
  use crate::utils::NoProxy;
  use crate::utils::url::ProxyProvider;

  use super::AgentStore;
  use super::RealUrlDownloader;

  #[test]
  fn test_agent_store() {
    struct TestProxyProvider;
    impl ProxyProvider for TestProxyProvider {
      fn get_proxy(&self, _kind: super::AgentKind) -> Option<&'static str> {
        Some("user:p@ssw0rd@localhost:9999")
      }
    }

    let logger = Arc::new(Logger::new(&LoggerOptions {
      initial_context_name: "test".to_string(),
      is_stdout_machine_readable: false,
      log_level: LogLevel::Debug,
    }));
    let agent_store = AgentStore {
      agents: Default::default(),
      logger: logger,
      no_proxy: NoProxy::from_string("dprint.dev"),
      proxy_url_provider: TestProxyProvider,
      unsafely_ignore_certificates: None,
    };
    let get = |url: &str, proxy: DownloadProxy<'_>| agent_store.get(super::AgentKind::Http, &url.parse().unwrap(), proxy);
    // the host, port and password of the proxy the agent was configured with
    let proxy_of = |agent: &ureq::Agent| {
      agent
        .config()
        .proxy()
        .map(|proxy| (proxy.host().to_string(), proxy.port(), proxy.password().map(|p| p.to_string())))
    };

    let agent = get("http://example.com", DownloadProxy::Environment).unwrap();
    assert_eq!(agent.proxy.as_deref(), Some("user:p@ssw0rd@localhost:9999"));
    assert_eq!(proxy_of(&agent.agent), Some(("localhost".to_string(), 9999, Some("p@ssw0rd".to_string()))));
    let agent2 = get("http://other.com", DownloadProxy::Environment).unwrap();
    assert_eq!(proxy_of(&agent2.agent), proxy_of(&agent.agent));
    assert_eq!(agent_store.agents.lock().len(), 1);

    let agent3 = get("http://dprint.dev", DownloadProxy::Environment).unwrap();
    assert_eq!(agent3.proxy, None);
    assert_eq!(proxy_of(&agent3.agent), None);

    // a proxy for the request takes the place of the environment's, but
    // not for a host that's excluded from being proxied
    let other_proxy = DownloadProxy::Url("https://other-proxy:8080");
    let agent4 = get("http://example.com", other_proxy).unwrap();
    assert_eq!(agent4.proxy.as_deref(), Some("https://other-proxy:8080"));
    assert_eq!(proxy_of(&agent4.agent), Some(("other-proxy".to_string(), 8080, None)));
    let agent5 = get("http://dprint.dev", other_proxy).unwrap();
    assert_eq!(agent5.proxy, None);

    // a direct request doesn't go through the environment's proxy
    let agent6 = get("http://example.com", DownloadProxy::Direct).unwrap();
    assert_eq!(agent6.proxy, None);
    assert_eq!(proxy_of(&agent6.agent), None);

    // the credentials aren't shown when the proxy can't be used
    let err = get("http://example.com", DownloadProxy::Url("ftp://user:p%40ssw0rd@other-proxy:8080"))
      .err()
      .unwrap();
    assert_eq!(format!("{:#}", err), "Invalid proxy ftp://other-proxy:8080.");

    // an https proxy is connected to over TLS, so the agent for an http
    // url needs the certificates as well
    assert!(matches!(agent4.agent.config().tls_config().root_certs(), ureq::tls::RootCerts::Specific(_)));
  }

  #[test]
  fn parses_proxies() {
    use super::parse_proxy;
    use ureq::ProxyProtocol;

    let parse = |text: &str| {
      let proxy = parse_proxy(text).unwrap();
      (
        proxy.protocol(),
        proxy.host().to_string(),
        proxy.port(),
        proxy.username().map(|v| v.to_string()),
        proxy.password().map(|v| v.to_string()),
      )
    };
    let no_auth = |protocol, host: &str, port| (protocol, host.to_string(), port, None, None);

    assert_eq!(parse("proxy.corp:8080"), no_auth(ProxyProtocol::Http, "proxy.corp", 8080));
    assert_eq!(parse("proxy.corp:8080/"), no_auth(ProxyProtocol::Http, "proxy.corp", 8080));
    assert_eq!(parse("HTTP://proxy.corp:8080/"), no_auth(ProxyProtocol::Http, "proxy.corp", 8080));
    assert_eq!(parse("https://proxy.corp"), no_auth(ProxyProtocol::Https, "proxy.corp", 443));
    assert_eq!(parse("socks4://proxy.corp"), no_auth(ProxyProtocol::Socks4, "proxy.corp", 1080));
    // the proxy resolves the host
    assert_eq!(parse("socks5://proxy.corp:1081"), no_auth(ProxyProtocol::Socks5h, "proxy.corp", 1081));
    assert_eq!(parse("socks://proxy.corp"), no_auth(ProxyProtocol::Socks5h, "proxy.corp", 1080));
    assert_eq!(
      parse("http://user:p@ss@proxy.corp:8080"),
      (
        ProxyProtocol::Http,
        "proxy.corp".to_string(),
        8080,
        Some("user".to_string()),
        Some("p@ss".to_string())
      )
    );

    // credentials that can't be provided to ureq, without showing them
    for text in [
      "http://DOMAIN\\user:pass@proxy.corp:8080",
      "http://user:p ss@proxy.corp:8080",
      "http://user:p/ss@proxy.corp:8080",
      "http://user:p?ss@proxy.corp:8080",
      "http://user:p#ss@proxy.corp:8080",
    ] {
      assert_eq!(
        parse_proxy(text).err().unwrap().to_string(),
        "Invalid proxy http://proxy.corp:8080. Its username or password may contain a character that is not supported.",
        "{}",
        text
      );
    }
    assert_eq!(parse_proxy("ftp://proxy.corp").err().unwrap().to_string(), "Invalid proxy ftp://proxy.corp.");
  }

  #[test]
  fn agent_store_ignores_certificates_per_host() {
    let agent_store = AgentStore {
      agents: Default::default(),
      logger: Arc::new(Logger::new(&LoggerOptions {
        initial_context_name: "test".to_string(),
        is_stdout_machine_readable: false,
        log_level: LogLevel::Silent,
      })),
      no_proxy: NoProxy::from_string("*"),
      proxy_url_provider: super::RealProxyUrlProvider,
      unsafely_ignore_certificates: Some(super::UnsafelyIgnoreCertificates::new(vec!["ignored.com".to_string(), "::1".to_string()])),
    };
    let is_verification_disabled = |kind: super::AgentKind, url: &str| {
      let agent = agent_store.get(kind, &url.parse().unwrap(), DownloadProxy::Environment).unwrap().agent;
      agent.config().tls_config().disable_verification()
    };

    assert!(is_verification_disabled(super::AgentKind::Https, "https://ignored.com/file"));
    assert!(is_verification_disabled(super::AgentKind::Https, "https://[::1]/file"));
    assert!(!is_verification_disabled(super::AgentKind::Https, "https://sub.ignored.com/file"));
    assert!(!is_verification_disabled(super::AgentKind::Https, "https://other.com/file"));
    assert!(!is_verification_disabled(super::AgentKind::Http, "http://ignored.com/file"));

    assert!(super::UnsafelyIgnoreCertificates::new(vec![]).ignores_host("other.com"));
  }

  #[test]
  fn request_error_messages() {
    use super::get_request_error_message;

    let url = "https://registry.npmjs.org/@dprint/exec".parse().unwrap();
    let proxy = Some("http://user:p@ssw0rd@proxy.corp:8080/");
    let connect_timeout = ureq::Error::Timeout(ureq::Timeout::Connect);
    assert_eq!(
      get_request_error_message(&url, &connect_timeout, None),
      "Timed out connecting to registry.npmjs.org."
    );
    assert_eq!(
      get_request_error_message(&url, &connect_timeout, proxy),
      "Timed out connecting to registry.npmjs.org through the proxy http://proxy.corp:8080."
    );
    assert_eq!(
      get_request_error_message(&url, &ureq::Error::Timeout(ureq::Timeout::RecvResponse), None),
      "Timed out waiting for a response from registry.npmjs.org."
    );
    // the text ureq provides when a proxy doesn't accept the request
    let proxy_failed = |status: u16| ureq::Error::ConnectProxyFailed(format!("proxy server responded {0}/{0}", status));
    assert_eq!(
      get_request_error_message(&url, &proxy_failed(407), proxy),
      concat!(
        "Could not connect to registry.npmjs.org through the proxy http://proxy.corp:8080: ",
        "the proxy requires authentication or rejected the credentials."
      )
    );
    assert_eq!(
      get_request_error_message(&url, &proxy_failed(403), proxy),
      "Could not connect to registry.npmjs.org through the proxy http://proxy.corp:8080: proxy server responded 403/403."
    );

    // a host that can't be resolved is one of these
    let not_found = || ureq::Error::from(std::io::Error::other("No such host is known."));
    assert_eq!(get_request_error_message(&url, &not_found(), None), "No such host is known.");
    assert_eq!(
      get_request_error_message(&url, &not_found(), proxy),
      "No such host is known. (using the proxy http://proxy.corp:8080)"
    );
    assert_eq!(get_request_error_message(&url, &ureq::Error::HostNotFound, None), "host not found");

    assert_eq!(super::display_proxy("user:p@ssw0rd@localhost:9999"), "localhost:9999");
    assert_eq!(super::display_proxy("socks5://localhost:9999"), "socks5://localhost:9999");
  }

  #[test]
  fn request_error_message_when_cannot_connect() {
    let downloader = create_direct_downloader();
    // bind then drop a listener to get a port nothing is listening on
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}/file", port);
    let message = downloader.download_no_retries_for_testing(&url).unwrap_err().to_string();
    let expected_start = format!("Error downloading {} - Could not connect to 127.0.0.1: ", url);
    assert!(message.starts_with(&expected_start), "{}", message);
  }

  #[test]
  fn downloads_from_server() {
    use std::io::BufRead;
    use std::io::Write;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
      for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut lines = Vec::new();
        loop {
          let mut line = String::new();
          if reader.read_line(&mut line).unwrap() == 0 || line.trim().is_empty() {
            break;
          }
          lines.push(line.trim().to_string());
        }
        let authorization = lines
          .iter()
          .filter_map(|line| line.split_once(": "))
          .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
          .map(|(_, value)| value.to_string())
          .unwrap_or_default();
        let response = match lines[0].split(' ').nth(1).unwrap() {
          "/ok" => "200 OK\r\nContent-Length: 2\r\nX-Test: value\r\n\r\nHi".to_string(),
          "/auth" => format!("200 OK\r\nContent-Length: {}\r\n\r\n{}", authorization.len(), authorization),
          "/redirect" => "302 Found\r\nLocation: /ok\r\nContent-Length: 0\r\n\r\n".to_string(),
          "/redirect-nowhere" => "302 Found\r\nContent-Length: 0\r\n\r\n".to_string(),
          "/forbidden" => "403 Forbidden\r\nContent-Length: 4\r\n\r\nNope".to_string(),
          _ => "404 Not Found\r\nContent-Length: 0\r\n\r\n".to_string(),
        };
        // the client may have given up on the request
        _ = stream.write_all(format!("HTTP/1.1 {}", response.replacen("\r\n", "\r\nConnection: close\r\n", 1)).as_bytes());
      }
    });

    let downloader = create_direct_downloader();
    let download = |path: &str, auth: Option<&str>| {
      let url = format!("{}{}", origin, path).parse().unwrap();
      downloader.download(&url, DownloadOptions { auth, ..Default::default() })
    };

    let file = download("/ok", None).unwrap().unwrap();
    assert_eq!(file.content, b"Hi");
    assert_eq!(file.headers.get("x-test").map(|v| v.as_str()), Some("value"));
    assert_eq!(download("/auth", Some("Bearer T")).unwrap().unwrap().content, b"Bearer T");
    assert_eq!(download("/auth", None).unwrap().unwrap().content, b"");

    // redirects are left for the caller to follow
    let file = download("/redirect", None).unwrap().unwrap();
    assert_eq!(file.headers.get("location").map(|v| v.as_str()), Some("/ok"));
    assert_eq!(file.content, b"");

    assert_eq!(
      download("/redirect-nowhere", None).err().unwrap().to_string(),
      format!("Error downloading {}/redirect-nowhere - 302 without a location to redirect to", origin)
    );

    assert!(download("/missing", None).unwrap().is_none());
    assert_eq!(
      download("/forbidden", None).err().unwrap().to_string(),
      format!("Error downloading {}/forbidden - 403 Forbidden", origin)
    );
  }

  fn create_direct_downloader() -> RealUrlDownloader {
    RealUrlDownloader::new(
      None,
      Arc::new(Logger::new(&LoggerOptions {
        initial_context_name: "dprint".to_string(),
        is_stdout_machine_readable: true,
        log_level: LogLevel::Silent,
      })),
      NoProxy::from_string("*"),
      None,
    )
    .unwrap()
  }

  #[test]
  fn unsafe_ignore_cert() {
    fn create_downloader(ignore_option: Option<Vec<String>>) -> RealUrlDownloader {
      RealUrlDownloader::new(
        None,
        Arc::new(Logger::new(&LoggerOptions {
          initial_context_name: "dprint".to_string(),
          is_stdout_machine_readable: true,
          log_level: LogLevel::Silent,
        })),
        NoProxy::from_string(""),
        ignore_option.map(|value| super::UnsafelyIgnoreCertificates(Arc::new(value))),
      )
      .unwrap()
    }

    let Some(_server) = start_deno_server() else {
      return; // ignore if the person running the test suite doesn't have Deno installed
    };

    // wait for the server to start
    {
      let downloader = create_downloader(Some(vec![]));
      for i in 1..=10 {
        let result = downloader.download_no_retries_for_testing("https://localhost:8063");
        if result.is_ok() {
          break;
        } else {
          std::thread::sleep(Duration::from_millis(10 * i));
        }
      }
    }

    // allow all
    {
      let downloader = create_downloader(Some(vec![]));
      let value = downloader.download_no_retries_for_testing("https://localhost:8063").unwrap().unwrap();
      assert_eq!(value, "Hi".as_bytes().to_vec());
    }
    // right host
    {
      let downloader = create_downloader(Some(vec!["localhost".to_string()]));
      let value = downloader.download_no_retries_for_testing("https://localhost:8063").unwrap().unwrap();
      assert_eq!(value, "Hi".as_bytes().to_vec());
    }
    // right ip
    {
      let downloader = create_downloader(Some(vec!["127.0.0.1".to_string()]));
      let value = downloader.download_no_retries_for_testing("https://127.0.0.1:8063").unwrap().unwrap();
      assert_eq!(value, "Hi".as_bytes().to_vec());
    }
    // not specified host
    {
      let downloader = create_downloader(Some(vec!["google.com".to_string()]));
      let result = downloader.download_no_retries_for_testing("https://localhost:8063");
      assert!(result.is_err());
    }
    // not specified ip
    {
      let downloader = create_downloader(Some(vec!["1.1.1.1".to_string()]));
      let result = downloader.download_no_retries_for_testing("https://localhost:8063");
      assert!(result.is_err());
    }
    // not configured, error
    {
      let downloader = create_downloader(None);
      let result = downloader.download_no_retries_for_testing("https://localhost:8063");
      assert!(result.is_err());
    }
  }

  struct ChildDrop {
    child: Child,
  }

  impl Drop for ChildDrop {
    fn drop(&mut self) {
      _ = self.child.kill();
    }
  }

  fn start_deno_server() -> Option<ChildDrop> {
    let cert = "-----BEGIN CERTIFICATE-----
MIIC+zCCAeOgAwIBAgIJAOFEwE15PYGsMA0GCSqGSIb3DQEBCwUAMBQxEjAQBgNV
BAMMCWxvY2FsaG9zdDAeFw0yNTAyMDEyMzE3MzFaFw0yNjAyMDEyMzE3MzFaMBQx
EjAQBgNVBAMMCWxvY2FsaG9zdDCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoC
ggEBAOeJ3ccDrg9MqBblIzEg+3J4DQJP2t1jHLapX/KjFY4tj1M5m9s9tNyRYDOk
4hhrXpWcOBJ3WvAt4MBgeP0rMP84j9CCH54i58SGJ8SZcvDGODjzwBpl1kks7oAT
CyftJlcpyY+oRcAFhKNz1WLLkm6gXiz9zv8KAd+tz9zlALdoafZteYiqSSwC9JpM
rkE908pJGvVkcpXZyQSxtNasB8W8Be3ZDj05z/dOugNtjssQqw3eGZlIFuIHrWmE
qvnz+VELd+14SgxWidf4QTtfvl1PFDbwysGBdu0sGeNnROTS9gILQDeIH4pbhk6z
L+HPAFYEONJuUTkbH+CQVcHw4BsCAwEAAaNQME4wHQYDVR0OBBYEFODfoAzFiSif
wMW//zOVH9cL8y/RMB8GA1UdIwQYMBaAFODfoAzFiSifwMW//zOVH9cL8y/RMAwG
A1UdEwQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEBAEWXZTIvSObeigjVzQVLiu94
7J5e9ab6MCMsEoj0+F5ZoTnPqYyvp7wyTARZXw84xxKMink0MF9PZzQj7QgTaPJf
G44K4GihZIPcSe0dZ9xZ3xdOmZAVG7zG3JLr/z+Ii2QcWfFB+SrqXVMHtXQtpCo7
W+y72MIkho2wTcuZWNB+cPQXZIILVXFMrB+6zLFjg9+TwcBgnAZhmstZqw4E8FZN
DdxDL9/wuh+uAGgx5pLnpL8aeZoIiDl+FiQ3tI3YU/EE6YC0Q6ky1t1psOwsEWyr
p6EkSRnEWbe+XxT71f2xHp1HbA7CZoiQnN4yU3UPQEIfMq3zFJYKnlc9CRmHgns=
-----END CERTIFICATE-----
";
    let key = "-----BEGIN PRIVATE KEY-----
MIIEvwIBADANBgkqhkiG9w0BAQEFAASCBKkwggSlAgEAAoIBAQDnid3HA64PTKgW
5SMxIPtyeA0CT9rdYxy2qV/yoxWOLY9TOZvbPbTckWAzpOIYa16VnDgSd1rwLeDA
YHj9KzD/OI/Qgh+eIufEhifEmXLwxjg488AaZdZJLO6AEwsn7SZXKcmPqEXABYSj
c9Viy5JuoF4s/c7/CgHfrc/c5QC3aGn2bXmIqkksAvSaTK5BPdPKSRr1ZHKV2ckE
sbTWrAfFvAXt2Q49Oc/3TroDbY7LEKsN3hmZSBbiB61phKr58/lRC3fteEoMVonX
+EE7X75dTxQ28MrBgXbtLBnjZ0Tk0vYCC0A3iB+KW4ZOsy/hzwBWBDjSblE5Gx/g
kFXB8OAbAgMBAAECggEBAJeqblS7q1uoOf7tT3USBsN/sf3Osy4LizZ3kjsM6sS8
QUMh3F7rd7p3m82YduXKByX3M5+dATuMwckiKH6luS2lLkdFxVI/yROpUQlt/qWL
Ii7kM/TWulwqi3vnfYpExLWZ0MdCUZYrxyuOZ7uUX7IJaEcOZnYXZwzO/PbUJvj7
tGAOwIDHe9e/FYPbTQSErkbMui5loyloL6K7R/RKQWxcB3iWHNutdceXr8EdwiBw
Ac2LYkt4f+vkm2/8dIfwIwxvjNSBzl/AHYRGJbWbrrP4J7VKJyBn0mdgnPy4+BfM
RJIUJMRrYFCu3GPtC2IvEUsUJk7dVZ+HUxVYEQyXM5ECgYEA9AF1eh+S+WT5TUTI
iSgVUyNg1yFAb6hggCdAH1BmfvwZfWmyLL4WPrjAgSdls88J/HvJWyrLQlhk9Z0U
5JkKuClNYEFwTYmhvMVQ7mFDfsxUfUURvKSOjTaS5iI/z5jGB4R5DrxAgRkgoz3/
KHwi3hOPErrXA57IaCZw+FEeWEMCgYEA8uuFpbyW+hnTvljPHeC0gs1IBLGxCn0m
/AELmFRvTaCwHN/VrOtOU+SsY3f8meS9DRqlcG6aJkxvzRD2QcgOEn0dtP2KTEFC
/sTbolUw9QVP/IujAHpB6pUuCGxELcAYSJmzqpl4pSOG126a84OX/igda3zF51gp
BLWvVeASp0kCgYEAnJP/FdIDF4TDMeFMqi8NmB8guow89CnhWvtU+4M1cpFFriPQ
UUPdtHwMFBT6/2qBZwLsUFNiwX1FtBML4DGRHmJqo7T6YtdJ8X/REldZ35kxMn3L
Bvm1/Eoj9AfQWOAZW6OXp2wIHI/KUNas0QbvvQBiFEvPRCR1R9g7MC2lwk8CgYEA
koWxZVitkEmHyKZ0t0bUWplLuVkcuoDmxNY0kjtLr30e/SueDOEZq8yglpbHDGRG
C+NoqrprzHIKdZynjOIIauqAwqyzgG9U46sF95J/Jyt/JYtsVFtp6v70dywmq5nU
i+X50wsjFCirqsISQJO9WBYGONFX5cTtaOPV0GyJk9ECgYBJtfhIdA+DagWWe0kF
ejEnS6W1Hid3gK0vnDVL6Fws3GXSxifw+XeI+LzOFCHovc6eExWF1qxyRDwi96l3
SUHki7X8yemi+g10U4xJWZcQkbkivDuGLopt87f1BHmy/1O2pFmMwh7+cVQIpm1l
kGUMOx8j0U5fU8eSLECGi0FxBA==
-----END PRIVATE KEY-----
";
    let result = Command::new("deno")
      .args([
        "eval".to_string(),
        format!("Deno.serve({{ port: 8063, cert: `{cert}`, key: `{key}` }}, req => new Response('Hi'));"),
      ])
      .stderr(Stdio::null())
      .stdout(Stdio::null())
      .spawn();
    match result {
      Ok(child) => Some(ChildDrop { child }),
      Err(err) => {
        if err.kind() == ErrorKind::NotFound {
          return None;
        } else {
          panic!("Failed running Deno: {:#}", err);
        }
      }
    }
  }
}

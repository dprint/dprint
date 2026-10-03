use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Result;
use anyhow::bail;
use deno_terminal::colors;
use parking_lot::Mutex;
use url::Url;

use self::unsafe_certs::NoCertificateVerification;

use super::Logger;
use super::certs::get_root_cert_store;
use super::logging::ProgressBarStyle;
use super::logging::ProgressBars;
use super::no_proxy::NoProxy;
use crate::environment::DownloadOptions;
use crate::environment::DownloadProxy;
use crate::environment::DownloadedFile;

const MAX_RETRIES: u8 = 2;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait on a server that was connected to for each part of its response.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

pub struct RealUrlDownloader {
  progress_bars: Option<Arc<ProgressBars>>,
  client_store: Arc<ClientStore<RealProxyUrlProvider>>,
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
      client_store: Arc::new(ClientStore {
        clients: Default::default(),
        logger: logger.clone(),
        no_proxy,
        proxy_url_provider: RealProxyUrlProvider,
        unsafely_ignore_certificates,
      }),
      logger,
    })
  }

  pub async fn download(&self, url: &Url, options: DownloadOptions<'_>) -> Result<Option<DownloadedFile>> {
    let client = self.get_client(url, options.proxy).await?;
    let mut last_error = None;
    for retry_count in 0..(MAX_RETRIES + 1) {
      match self.inner_download(url, options.auth, retry_count, &client).await {
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
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
      let client = self.get_client(&url, DownloadProxy::Environment).await?;
      Ok(self.inner_download(&url, None, 0, &client).await?.map(|r| r.content))
    })
  }

  async fn get_client(&self, url: &Url, proxy: DownloadProxy<'_>) -> Result<ClientWithProxy> {
    let kind = match url.scheme() {
      "https" => UrlKind::Https,
      "http" => UrlKind::Http,
      _ => bail!("Not implemented url scheme: {}", url),
    };
    let proxy = self.client_store.resolve_proxy(kind, url, proxy);
    // creating a client is expensive because it loads the certificates
    let client_store = self.client_store.clone();
    dprint_core::async_runtime::spawn_blocking(move || client_store.get(proxy)).await?
  }

  async fn inner_download(&self, url: &Url, auth: Option<&str>, retry_count: u8, client: &ClientWithProxy) -> Result<Option<DownloadedFile>> {
    let mut request = client.client.get(url.clone());
    if let Some(auth) = auth {
      request = request.header(reqwest::header::AUTHORIZATION, auth);
    }
    let mut resp = match request.send().await {
      Ok(resp) => resp,
      Err(err) => {
        bail!("Error downloading {} - {}", url, get_request_error_message(url, &err, client.proxy.as_deref()))
      }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND {
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
    let mut content = Vec::new();
    content.try_reserve_exact(total_size)?;
    let progress = self.progress_bars.as_ref().map(|progress_bars| {
      let mut message = format!("Downloading {}", url);
      if retry_count > 0 {
        message.push_str(&format!(" (Retry {}/{})", retry_count, MAX_RETRIES))
      }
      progress_bars.add_progress(message, ProgressBarStyle::Download, total_size)
    });
    loop {
      match resp.chunk().await {
        Ok(Some(chunk)) => {
          content.extend_from_slice(&chunk);
          if let Some(progress) = &progress {
            progress.set_position(content.len());
          }
        }
        Ok(None) => break,
        Err(err) => {
          bail!("Error downloading {} - {}", url, get_request_error_message(url, &err, client.proxy.as_deref()))
        }
      }
    }
    if let Some(progress) = progress {
      progress.finish();
    }
    Ok(Some(DownloadedFile { headers, content }))
  }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum UrlKind {
  Http,
  Https,
}

trait ProxyProvider {
  fn get_proxy(&self, kind: UrlKind) -> Option<&'static str>;
}

struct RealProxyUrlProvider;

impl ProxyProvider for RealProxyUrlProvider {
  fn get_proxy(&self, kind: UrlKind) -> Option<&'static str> {
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
      UrlKind::Http => HTTP_PROXY.get_or_init(|| read_proxy_env_var("HTTP_PROXY")).as_deref(),
      UrlKind::Https => HTTPS_PROXY.get_or_init(|| read_proxy_env_var("HTTPS_PROXY")).as_deref(),
    }
  }
}

#[derive(Clone)]
struct ClientWithProxy {
  client: reqwest::Client,
  /// The proxy the client sends its requests through.
  proxy: Option<String>,
}

struct ClientStore<TProxyUrlProvider: ProxyProvider> {
  clients: Mutex<HashMap<Option<String>, reqwest::Client>>,
  logger: Arc<Logger>,
  no_proxy: NoProxy,
  proxy_url_provider: TProxyUrlProvider,
  unsafely_ignore_certificates: Option<UnsafelyIgnoreCertificates>,
}

impl<TProxyUrlProvider: ProxyProvider> ClientStore<TProxyUrlProvider> {
  /// Gets the proxy to request the url through, if any.
  pub fn resolve_proxy(&self, kind: UrlKind, url: &Url, proxy: DownloadProxy<'_>) -> Option<String> {
    let proxy = match proxy {
      DownloadProxy::Environment => self.proxy_url_provider.get_proxy(kind),
      DownloadProxy::Url(proxy) => Some(proxy),
      DownloadProxy::Direct => None,
    };
    let proxy = proxy.filter(|_| match url.host_str() {
      Some(host) => !self.no_proxy.contains(host),
      None => true,
    });
    proxy.map(|proxy| proxy.to_string())
  }

  /// Gets the client that sends its requests through the proxy.
  pub fn get(&self, proxy: Option<String>) -> Result<ClientWithProxy> {
    let mut clients = self.clients.lock();
    let client = match clients.get(&proxy) {
      Some(client) => client.clone(),
      None => {
        // blocking the lock isn't too bad here because generally
        // there will only ever be one of these created ever
        let client = self.build_client(proxy.as_deref())?;
        clients.insert(proxy.clone(), client.clone());
        client
      }
    };
    Ok(ClientWithProxy { client, proxy })
  }

  fn build_client(&self, proxy: Option<&str>) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
      // redirects are handled by the downloader
      .redirect(reqwest::redirect::Policy::none())
      .connect_timeout(CONNECT_TIMEOUT)
      .read_timeout(READ_TIMEOUT)
      // the certificates are needed even for an http url because the
      // connection to an https proxy is made over TLS as well
      .tls_backend_preconfigured(self.build_tls_config()?);
    let builder = match proxy {
      Some(proxy) => builder.proxy(parse_proxy(proxy)?),
      // reqwest otherwise reads the proxy from the environment itself
      None => builder.no_proxy(),
    };
    Ok(builder.build()?)
  }

  fn build_tls_config(&self) -> Result<rustls::ClientConfig> {
    static INSTALLED_PROVIDER: std::sync::OnceLock<()> = std::sync::OnceLock::new();
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
    let root_store = Arc::new(get_root_cert_store(&self.logger, &|env_var| std::env::var(env_var).ok(), &|file_path| {
      std::fs::read(file_path)
    })?);
    let mut config = rustls::ClientConfig::builder().with_root_certificates(root_store.clone()).with_no_client_auth();
    if let Some(unsafe_certificates) = &self.unsafely_ignore_certificates {
      config
        .dangerous()
        .set_certificate_verifier(Arc::new(NoCertificateVerification::new(unsafe_certificates.0.clone(), root_store)?));
    }
    Ok(config)
  }
}

/// Creates a proxy from the text of a proxy setting, which has the
/// form `<protocol>://<user>:<password>@<host>:<port>` where everything
/// but the host is optional.
fn parse_proxy(text: &str) -> Result<reqwest::Proxy> {
  let trimmed_text = text.trim_end_matches('/');
  let (scheme, rest) = match trimmed_text.split_once("://") {
    Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
    None => ("http".to_string(), trimmed_text),
  };
  let scheme = match scheme.as_str() {
    // have the proxy resolve the host instead of resolving it locally, which
    // is what a socks5 proxy has always been used for here
    "socks" | "socks5" => "socks5h",
    "http" | "https" | "socks4" | "socks4a" | "socks5h" => scheme.as_str(),
    _ => bail!("Invalid proxy {}. Its scheme is not supported.", display_proxy(text)),
  };
  // the credentials are provided separately from the url so that
  // they may contain characters that aren't allowed in one
  let (credentials, address) = match rest.rsplit_once('@') {
    Some((credentials, address)) => (Some(credentials), address),
    None => (None, rest),
  };
  let Ok(proxy) = reqwest::Proxy::all(format!("{}://{}", scheme, address)) else {
    bail!("Invalid proxy {}.", display_proxy(text));
  };
  Ok(match credentials {
    Some(credentials) => {
      let (username, password) = credentials.split_once(':').unwrap_or((credentials, ""));
      proxy.basic_auth(username, password)
    }
    None => proxy,
  })
}

/// Describes why a request failed without repeating the url, which reqwest's
/// own message does.
fn get_request_error_message(url: &Url, err: &reqwest::Error, proxy: Option<&str>) -> String {
  let host = url.host_str().unwrap_or(url.as_str());
  let proxy = proxy.map(display_proxy);
  let target = match &proxy {
    Some(proxy) => format!("{} through the proxy {}", host, proxy),
    None => host.to_string(),
  };
  if err.is_timeout() {
    if err.is_connect() {
      format!("Timed out connecting to {}.", target)
    } else {
      format!("Timed out waiting for a response from {}.", target)
    }
  } else if err.is_connect() {
    format!("Could not connect to {}: {}.", target, get_root_cause_text(err))
  } else {
    match &proxy {
      Some(proxy) => format!("{} (using the proxy {})", get_root_cause_text(err), proxy),
      None => get_root_cause_text(err),
    }
  }
}

/// reqwest's errors wrap the one that says what went wrong.
fn get_root_cause_text(err: &reqwest::Error) -> String {
  let mut cause: &dyn std::error::Error = err;
  while let Some(source) = cause.source() {
    cause = source;
  }
  cause.to_string().trim_end_matches('.').to_string()
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
}

mod unsafe_certs {
  use std::net::IpAddr;
  use std::sync::Arc;

  use rustls::DigitallySignedStruct;
  use rustls::RootCertStore;
  use rustls::client::WebPkiServerVerifier;
  use rustls::client::danger::HandshakeSignatureValid;
  use rustls::client::danger::ServerCertVerified;
  use rustls::client::danger::ServerCertVerifier;
  use rustls::pki_types::ServerName;
  use rustls::server::VerifierBuilderError;

  // Below code copied and adapted from https://github.com/denoland/deno/blob/540fe7d9e46d6e734af1ce737adf90e8fc00dff8/ext/tls/lib.rs#L68
  // Copyright 2018-2025 the Deno authors. MIT license.

  #[derive(Debug)]
  pub struct NoCertificateVerification {
    ic_allowlist: Arc<Vec<String>>,
    default_verifier: Arc<WebPkiServerVerifier>,
  }

  impl NoCertificateVerification {
    pub fn new(ic_allowlist: Arc<Vec<String>>, root_cert_store: Arc<RootCertStore>) -> Result<Self, VerifierBuilderError> {
      Ok(Self {
        ic_allowlist,
        default_verifier: WebPkiServerVerifier::builder(root_cert_store).build()?,
      })
    }
  }

  impl ServerCertVerifier for NoCertificateVerification {
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
      self.default_verifier.supported_verify_schemes()
    }

    fn verify_server_cert(
      &self,
      end_entity: &rustls::pki_types::CertificateDer<'_>,
      intermediates: &[rustls::pki_types::CertificateDer<'_>],
      server_name: &rustls::pki_types::ServerName<'_>,
      ocsp_response: &[u8],
      now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
      if self.ic_allowlist.is_empty() {
        return Ok(ServerCertVerified::assertion());
      }
      let dns_name_or_ip_address = match server_name {
        ServerName::DnsName(dns_name) => dns_name.as_ref().to_owned(),
        ServerName::IpAddress(ip_address) => Into::<IpAddr>::into(*ip_address).to_string(),
        _ => {
          // NOTE(bartlomieju): `ServerName` is a non-exhaustive enum
          // so we have this catch all errors here.
          return Err(rustls::Error::General("Unknown `ServerName` variant".to_string()));
        }
      };
      if self.ic_allowlist.contains(&dns_name_or_ip_address) {
        Ok(ServerCertVerified::assertion())
      } else {
        self
          .default_verifier
          .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
      }
    }

    fn verify_tls12_signature(
      &self,
      message: &[u8],
      cert: &rustls::pki_types::CertificateDer,
      dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
      if self.ic_allowlist.is_empty() {
        return Ok(HandshakeSignatureValid::assertion());
      }
      filter_invalid_encoding_err(self.default_verifier.verify_tls12_signature(message, cert, dss))
    }

    fn verify_tls13_signature(
      &self,
      message: &[u8],
      cert: &rustls::pki_types::CertificateDer,
      dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
      if self.ic_allowlist.is_empty() {
        return Ok(HandshakeSignatureValid::assertion());
      }
      filter_invalid_encoding_err(self.default_verifier.verify_tls13_signature(message, cert, dss))
    }
  }

  fn filter_invalid_encoding_err(to_be_filtered: Result<HandshakeSignatureValid, rustls::Error>) -> Result<HandshakeSignatureValid, rustls::Error> {
    match to_be_filtered {
      Err(rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding)) => Ok(HandshakeSignatureValid::assertion()),
      res => res,
    }
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

  use super::ClientStore;
  use super::RealUrlDownloader;
  use super::UrlKind;

  #[test]
  fn client_store_resolves_proxy() {
    struct TestProxyProvider;
    impl ProxyProvider for TestProxyProvider {
      fn get_proxy(&self, _kind: UrlKind) -> Option<&'static str> {
        Some("user:p@ssw0rd@localhost:9999")
      }
    }

    let client_store = ClientStore {
      clients: Default::default(),
      logger: Arc::new(Logger::new(&LoggerOptions {
        initial_context_name: "test".to_string(),
        is_stdout_machine_readable: false,
        log_level: LogLevel::Debug,
      })),
      no_proxy: NoProxy::from_string("dprint.dev"),
      proxy_url_provider: TestProxyProvider,
      unsafely_ignore_certificates: None,
    };
    let resolve = |url: &str, proxy: DownloadProxy<'_>| client_store.resolve_proxy(UrlKind::Http, &url.parse().unwrap(), proxy);
    let env_proxy = Some("user:p@ssw0rd@localhost:9999".to_string());

    assert_eq!(resolve("http://example.com", DownloadProxy::Environment), env_proxy);
    assert_eq!(resolve("http://other.com", DownloadProxy::Environment), env_proxy);
    assert_eq!(resolve("http://dprint.dev", DownloadProxy::Environment), None);

    // a proxy for the request takes the place of the environment's, but
    // not for a host that's excluded from being proxied
    let other_proxy = DownloadProxy::Url("https://other-proxy:8080");
    assert_eq!(resolve("http://example.com", other_proxy), Some("https://other-proxy:8080".to_string()));
    assert_eq!(resolve("http://dprint.dev", other_proxy), None);

    // a direct request doesn't go through the environment's proxy
    assert_eq!(resolve("http://example.com", DownloadProxy::Direct), None);

    // one client is created per proxy
    client_store.get(env_proxy.clone()).unwrap();
    client_store.get(env_proxy.clone()).unwrap();
    assert_eq!(client_store.clients.lock().len(), 1);
    client_store.get(None).unwrap();
    assert_eq!(client_store.get(None).unwrap().proxy, None);
    assert_eq!(client_store.clients.lock().len(), 2);

    // the credentials aren't shown when the proxy can't be used
    let err = client_store.get(Some("ftp://user:p@ssw0rd@other-proxy:8080".to_string())).err().unwrap();
    assert_eq!(format!("{:#}", err), "Invalid proxy ftp://other-proxy:8080. Its scheme is not supported.");
  }

  #[test]
  fn parses_proxies() {
    use super::parse_proxy;

    for text in [
      "proxy.corp:8080",
      "proxy.corp:8080/",
      "HTTP://proxy.corp:8080/",
      "https://proxy.corp",
      "socks4://proxy.corp",
      "socks5://proxy.corp:1081",
      "socks5h://[::1]:1081",
      "http://user:p@ss@proxy.corp:8080",
      "http://DOMAIN\\user:pass@proxy.corp:8080",
      "http://user:p /?#ss@proxy.corp:8080",
      "user@proxy.corp",
    ] {
      assert!(parse_proxy(text).is_ok(), "{}", text);
    }
    assert_eq!(
      parse_proxy("ftp://user:pass@proxy.corp").err().unwrap().to_string(),
      "Invalid proxy ftp://proxy.corp. Its scheme is not supported."
    );
    assert_eq!(
      parse_proxy("http://user:pass@proxy corp").err().unwrap().to_string(),
      "Invalid proxy http://proxy corp."
    );

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

  #[tokio::test]
  async fn downloads_from_server() {
    let origin = start_test_server(|path, headers| {
      let header = |name: &str| {
        headers
          .iter()
          .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
          .map(|(_, value)| value.to_string())
      };
      match path {
        "/ok" => "200 OK\r\nContent-Length: 2\r\nX-Test: value\r\n\r\nHi".to_string(),
        "/auth" => {
          let authorization = header("authorization").unwrap_or_default();
          format!("200 OK\r\nContent-Length: {}\r\n\r\n{}", authorization.len(), authorization)
        }
        "/redirect" => "302 Found\r\nLocation: /ok\r\nContent-Length: 0\r\n\r\n".to_string(),
        "/redirect-nowhere" => "302 Found\r\nContent-Length: 0\r\n\r\n".to_string(),
        "/forbidden" => "403 Forbidden\r\nContent-Length: 4\r\n\r\nNope".to_string(),
        _ => "404 Not Found\r\nContent-Length: 0\r\n\r\n".to_string(),
      }
    });

    let downloader = create_direct_downloader();
    let download = async |path: &str, auth: Option<&str>| {
      let url = format!("{}{}", origin, path).parse().unwrap();
      downloader.download(&url, DownloadOptions { auth, ..Default::default() }).await
    };

    let file = download("/ok", None).await.unwrap().unwrap();
    assert_eq!(file.content, b"Hi");
    assert_eq!(file.headers.get("x-test").map(|v| v.as_str()), Some("value"));
    assert_eq!(download("/auth", Some("Bearer T")).await.unwrap().unwrap().content, b"Bearer T");
    assert_eq!(download("/auth", None).await.unwrap().unwrap().content, b"");

    // redirects are left for the caller to follow
    let file = download("/redirect", None).await.unwrap().unwrap();
    assert_eq!(file.headers.get("location").map(|v| v.as_str()), Some("/ok"));
    assert_eq!(file.content, b"");
    assert_eq!(
      download("/redirect-nowhere", None).await.err().unwrap().to_string(),
      format!("Error downloading {}/redirect-nowhere - 302 without a location to redirect to", origin)
    );

    assert!(download("/missing", None).await.unwrap().is_none());
    assert_eq!(
      download("/forbidden", None).await.err().unwrap().to_string(),
      format!("Error downloading {}/forbidden - 403 Forbidden", origin)
    );
  }

  #[tokio::test]
  async fn downloads_through_http_proxy() {
    // a proxy that's sent a request for an http url forwards it, so act as
    // one by responding with what was requested and the credentials provided
    let proxy_origin = start_test_server(|path, headers| {
      let authorization = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("proxy-authorization"))
        .map(|(_, value)| value.to_string())
        .unwrap_or_default();
      let body = format!("{} {}", path, authorization);
      format!("200 OK\r\nContent-Length: {}\r\n\r\n{}", body.len(), body)
    });
    let proxy = format!("http://DOMAIN\\user:p@ss w0rd@{}", proxy_origin.strip_prefix("http://").unwrap());

    let downloader = create_downloader(NoProxy::from_string(""));
    let url = "http://example.invalid/file.wasm".parse().unwrap();
    let options = DownloadOptions {
      proxy: DownloadProxy::Url(&proxy),
      ..Default::default()
    };
    let file = downloader.download(&url, options).await.unwrap().unwrap();
    // base64 of `DOMAIN\user:p@ss w0rd`
    assert_eq!(
      String::from_utf8(file.content).unwrap(),
      "http://example.invalid/file.wasm Basic RE9NQUlOXHVzZXI6cEBzcyB3MHJk"
    );
  }

  /// Starts a server that responds to each request with the provided
  /// function's text, which is everything after the http version.
  fn start_test_server(respond: impl Fn(&str, &[(String, String)]) -> String + Send + 'static) -> String {
    use std::io::BufRead;
    use std::io::Write;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
      for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
          return;
        };
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut lines = Vec::new();
        loop {
          let mut line = String::new();
          if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
            break;
          }
          lines.push(line.trim().to_string());
        }
        let Some(path) = lines.first().and_then(|line| line.split(' ').nth(1)) else {
          continue;
        };
        let headers = lines[1..]
          .iter()
          .filter_map(|line| line.split_once(": "))
          .map(|(name, value)| (name.to_string(), value.to_string()))
          .collect::<Vec<_>>();
        let response = respond(path, &headers).replacen("\r\n", "\r\nConnection: close\r\n", 1);
        // the client may have given up on the request
        _ = stream.write_all(format!("HTTP/1.1 {}", response).as_bytes());
      }
    });
    origin
  }

  /// Creates a downloader that doesn't use the environment's proxy.
  fn create_direct_downloader() -> RealUrlDownloader {
    create_downloader(NoProxy::from_string("*"))
  }

  fn create_downloader(no_proxy: NoProxy) -> RealUrlDownloader {
    RealUrlDownloader::new(
      None,
      Arc::new(Logger::new(&LoggerOptions {
        initial_context_name: "dprint".to_string(),
        is_stdout_machine_readable: true,
        log_level: LogLevel::Silent,
      })),
      no_proxy,
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

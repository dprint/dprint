use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::Result;
use anyhow::bail;
use crossterm::style::Stylize;
use parking_lot::Mutex;
use url::Url;

use self::unsafe_certs::NoCertificateVerification;

use super::Logger;
use super::certs::get_root_cert_store;
use super::logging::ProgressBarStyle;
use super::logging::ProgressBars;
use super::no_proxy::NoProxy;

const MAX_RETRIES: u8 = 2;

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
        .and_then(|v| if v.is_empty() { None } else { Some(v) })
    }

    static HTTP_PROXY: OnceLock<Option<String>> = OnceLock::new();
    static HTTPS_PROXY: OnceLock<Option<String>> = OnceLock::new();

    match kind {
      AgentKind::Http => HTTP_PROXY.get_or_init(|| read_proxy_env_var("HTTP_PROXY")).as_deref(),
      AgentKind::Https => HTTPS_PROXY.get_or_init(|| read_proxy_env_var("HTTPS_PROXY")).as_deref(),
    }
  }
}

struct AgentStore<TProxyUrlProvider: ProxyProvider> {
  agents: Mutex<HashMap<(AgentKind, Option<&'static str>), ureq::Agent>>,
  logger: Arc<Logger>,
  no_proxy: NoProxy,
  proxy_url_provider: TProxyUrlProvider,
  unsafely_ignore_certificates: Option<UnsafelyIgnoreCertificates>,
}

impl<TProxyUrlProvider: ProxyProvider> AgentStore<TProxyUrlProvider> {
  pub fn get(&self, kind: AgentKind, url: &Url) -> Result<ureq::Agent> {
    let proxy = self.proxy_url_provider.get_proxy(kind);
    let proxy = proxy.filter(|_| match url.host_str() {
      Some(host) => !self.no_proxy.contains(host),
      None => true,
    });
    let key = (kind, proxy);
    let mut agents = self.agents.lock();
    let entry = agents.entry(key);
    Ok(match entry {
      std::collections::hash_map::Entry::Occupied(occupied_entry) => occupied_entry.get().clone(),
      std::collections::hash_map::Entry::Vacant(vacant_entry) => {
        // blocking the lock isn't too bad here because generally
        // there will only ever be one of these created ever
        let agent = self.build_agent(kind, proxy)?;
        vacant_entry.insert(agent.clone());
        agent
      }
    })
  }

  fn build_agent(&self, kind: AgentKind, proxy: Option<&str>) -> Result<ureq::Agent> {
    static INSTALLED_PROVIDER: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let mut agent = ureq::AgentBuilder::new();
    if kind == AgentKind::Https {
      INSTALLED_PROVIDER.get_or_init(|| {
        if let Some(ignored) = &self.unsafely_ignore_certificates {
          log_warn!(
            self.logger,
            "{} Unsafely ignoring {} TLS certificates!",
            "Warning".yellow(),
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
      agent = agent.tls_config(Arc::new(config));
    }
    if let Some(proxy) = proxy {
      agent = agent.proxy(ureq::Proxy::new(proxy)?);
    }
    Ok(agent.build())
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

  pub fn download(&self, url: &str) -> Result<Option<Vec<u8>>> {
    let (agent, url) = self.get_agent_and_url(url)?;
    self.download_with_retries(&url, &agent)
  }

  fn download_with_retries(&self, url: &Url, agent: &ureq::Agent) -> Result<Option<Vec<u8>>> {
    let mut last_error = None;
    for retry_count in 0..(MAX_RETRIES + 1) {
      match self.inner_download(url, retry_count, agent) {
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
    let (agent, url) = self.get_agent_and_url(url)?;
    self.inner_download(&url, 0, &agent)
  }

  fn get_agent_and_url(&self, url: &str) -> Result<(ureq::Agent, Url)> {
    let url = Url::parse(url)?;
    let kind = match url.scheme() {
      "https" => AgentKind::Https,
      "http" => AgentKind::Http,
      _ => bail!("Not implemented url scheme: {}", url),
    };
    // this is expensive, but we're already in a blocking task here
    let agent = self.agent_store.get(kind, &url)?;
    Ok((agent, url))
  }

  fn inner_download(&self, url: &Url, retry_count: u8, agent: &ureq::Agent) -> Result<Option<Vec<u8>>> {
    let resp = match agent.request_url("GET", url).call() {
      Ok(resp) => resp,
      Err(ureq::Error::Status(404, _)) => {
        return Ok(None);
      }
      Err(err) => {
        bail!("Error downloading {} - Error: {:#}", url, err)
      }
    };

    let total_size = resp.header("Content-Length").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let mut reader = resp.into_reader();
    match read_response(url, retry_count, &mut reader, total_size, self.progress_bars.as_deref()) {
      Ok(result) => Ok(Some(result)),
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

#[cfg(test)]
mod test {
  use std::io::ErrorKind;
  use std::process::Child;
  use std::process::Command;
  use std::process::Stdio;
  use std::sync::Arc;
  use std::time::Duration;

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

    let agent = agent_store.get(super::AgentKind::Http, &"http://example.com".parse().unwrap()).unwrap();
    let agent2 = agent_store.get(super::AgentKind::Http, &"http://other.com".parse().unwrap()).unwrap();
    assert_eq!(format!("{:?}", agent), format!("{:?}", agent2));
    assert!(format!("{:?}", agent).contains("p@ssw0rd"));

    let agent3 = agent_store.get(super::AgentKind::Http, &"http://dprint.dev".parse().unwrap()).unwrap();
    assert_ne!(format!("{:?}", agent), format!("{:?}", agent3));
    assert!(!format!("{:?}", agent3).contains("p@ssw0rd"));
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

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::bail;
use anyhow::Result;
use parking_lot::Mutex;
use url::Url;

use super::certs::get_root_cert_store;
use super::logging::ProgressBarStyle;
use super::logging::ProgressBars;
use super::no_proxy::NoProxy;
use super::Logger;

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
    let mut agent = ureq::AgentBuilder::new();
    if kind == AgentKind::Https {
      let previous_provider = rustls::crypto::ring::default_provider().install_default();
      debug_assert!(previous_provider.is_ok());

      #[allow(clippy::disallowed_methods)]
      let root_store = get_root_cert_store(&self.logger, &|env_var| std::env::var(env_var).ok(), &|file_path| std::fs::read(file_path))?;
      let config = rustls::ClientConfig::builder().with_root_certificates(root_store).with_no_client_auth();
      agent = agent.tls_config(Arc::new(config));
    }
    if let Some(proxy) = proxy {
      agent = agent.proxy(ureq::Proxy::new(proxy)?);
    }
    Ok(agent.build())
  }
}

pub struct RealUrlDownloader {
  progress_bars: Option<Arc<ProgressBars>>,
  agent_store: AgentStore<RealProxyUrlProvider>,
  logger: Arc<Logger>,
}

impl RealUrlDownloader {
  pub fn new(progress_bars: Option<Arc<ProgressBars>>, logger: Arc<Logger>, no_proxy: NoProxy) -> Result<Self> {
    Ok(Self {
      progress_bars,
      agent_store: AgentStore {
        agents: Default::default(),
        logger: logger.clone(),
        no_proxy,
        proxy_url_provider: RealProxyUrlProvider,
      },
      logger,
    })
  }

  pub fn download(&self, url: &str) -> Result<Option<Vec<u8>>> {
    let url = Url::parse(url)?;
    let kind = match url.scheme() {
      "https" => AgentKind::Https,
      "http" => AgentKind::Http,
      _ => bail!("Not implemented url scheme: {}", url),
    };
    // this is expensive, but we're already in a blocking task here
    let agent = self.agent_store.get(kind, &url)?;
    self.download_with_retries(&url, &agent)
  }

  fn download_with_retries(&self, url: &Url, agent: &ureq::Agent) -> Result<Option<Vec<u8>>> {
    let mut last_error = None;
    for retry_count in 0..(MAX_RETRIES + 1) {
      match inner_download(url, retry_count, agent, self.progress_bars.as_deref()) {
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
}

fn inner_download(url: &Url, retry_count: u8, agent: &ureq::Agent, progress_bars: Option<&ProgressBars>) -> Result<Option<Vec<u8>>> {
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
  match read_response(url, retry_count, &mut reader, total_size, progress_bars) {
    Ok(result) => Ok(Some(result)),
    Err(err) => bail!("Error downloading {} - {:#}", url, err),
  }
}

fn read_response(url: &Url, retry_count: u8, reader: &mut impl Read, total_size: usize, progress_bars: Option<&ProgressBars>) -> Result<Vec<u8>> {
  let mut final_bytes = Vec::with_capacity(total_size);
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
  use std::sync::Arc;

  use crate::utils::url::ProxyProvider;
  use crate::utils::LogLevel;
  use crate::utils::Logger;
  use crate::utils::LoggerOptions;
  use crate::utils::NoProxy;

  use super::AgentStore;

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
    };

    let agent = agent_store.get(super::AgentKind::Http, &"http://example.com".parse().unwrap()).unwrap();
    let agent2 = agent_store.get(super::AgentKind::Http, &"http://other.com".parse().unwrap()).unwrap();
    assert_eq!(format!("{:?}", agent), format!("{:?}", agent2));
    assert!(format!("{:?}", agent).contains("p@ssw0rd"));

    let agent3 = agent_store.get(super::AgentKind::Http, &"http://dprint.dev".parse().unwrap()).unwrap();
    assert_ne!(format!("{:?}", agent), format!("{:?}", agent3));
    assert!(!format!("{:?}", agent3).contains("p@ssw0rd"));
  }
}

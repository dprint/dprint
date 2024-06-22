use std::io::Read;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use once_cell::sync::OnceCell;

use super::certs::get_root_cert_store;
use super::logging::ProgressBarStyle;
use super::logging::ProgressBars;
use super::Logger;

const MAX_RETRIES: u8 = 2;

pub struct RealUrlDownloader {
  https_agent: OnceCell<ureq::Agent>,
  http_agent: OnceCell<ureq::Agent>,
  progress_bars: Option<Arc<ProgressBars>>,
  logger: Arc<Logger>,
}

impl RealUrlDownloader {
  pub fn new(progress_bars: Option<Arc<ProgressBars>>, logger: Arc<Logger>) -> Result<Self> {
    Ok(Self {
      https_agent: Default::default(),
      http_agent: Default::default(),
      progress_bars,
      logger,
    })
  }

  pub fn download(&self, url: &str) -> Result<Option<Vec<u8>>> {
    let lowercase_url = url.to_lowercase();
    let (agent, kind) = if lowercase_url.starts_with("https://") {
      (&self.https_agent, AgentKind::Https)
    } else if lowercase_url.starts_with("http://") {
      (&self.http_agent, AgentKind::Http)
    } else {
      bail!("Not implemented url scheme: {}", url);
    };
    // this is expensive, but we're already in a blocking task here
    let agent = agent.get_or_try_init(|| build_agent(kind, &self.logger))?;
    self.download_with_retries(url, agent)
  }

  fn download_with_retries(&self, url: &str, agent: &ureq::Agent) -> Result<Option<Vec<u8>>> {
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

fn inner_download(url: &str, retry_count: u8, agent: &ureq::Agent, progress_bars: Option<&ProgressBars>) -> Result<Option<Vec<u8>>> {
  let resp = match agent.get(url).call() {
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

fn read_response(url: &str, retry_count: u8, reader: &mut impl Read, total_size: usize, progress_bars: Option<&ProgressBars>) -> Result<Vec<u8>> {
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

#[derive(Copy, Clone, PartialEq, Eq)]
enum AgentKind {
  Http,
  Https,
}

fn build_agent(kind: AgentKind, logger: &Logger) -> Result<ureq::Agent> {
  let mut agent = ureq::AgentBuilder::new();
  if kind == AgentKind::Https {
    #[allow(clippy::disallowed_methods)]
    let root_store = get_root_cert_store(logger, &|env_var| std::env::var(env_var).ok(), &|file_path| std::fs::read(file_path))?;
    let config = rustls::ClientConfig::builder().with_root_certificates(root_store).with_no_client_auth();
    agent = agent.tls_config(Arc::new(config));
  }
  if let Some(proxy_url) = get_proxy_url(kind) {
    agent = agent.proxy(ureq::Proxy::new(proxy_url)?);
  }
  Ok(agent.build())
}

fn get_proxy_url(kind: AgentKind) -> Option<String> {
  match kind {
    AgentKind::Http => read_proxy_env_var("HTTP_PROXY"),
    AgentKind::Https => read_proxy_env_var("HTTPS_PROXY"),
  }
}

fn read_proxy_env_var(env_var_name: &str) -> Option<String> {
  // too much of a hassle to create a seam for the env var reading
  // and this struct is created before an env is created anyway
  #[allow(clippy::disallowed_methods)]
  std::env::var(env_var_name.to_uppercase())
    .ok()
    .or_else(|| std::env::var(env_var_name.to_lowercase()).ok())
}

use std::io::Read;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;

use super::logging::ProgressBarStyle;
use super::logging::ProgressBars;
use super::Logger;

const MAX_RETRIES: u8 = 2;

pub struct RealUrlDownloader {
  https_agent: ureq::Agent,
  http_agent: ureq::Agent,
  progress_bars: Option<Arc<ProgressBars>>,
  logger: Arc<Logger>,
}

impl RealUrlDownloader {
  pub fn new(progress_bars: Option<Arc<ProgressBars>>, logger: Arc<Logger>, read_env_var: impl Fn(&str) -> Option<String>) -> Result<Self> {
    Ok(Self {
      https_agent: build_agent(AgentKind::Https, &read_env_var)?,
      http_agent: build_agent(AgentKind::Http, &read_env_var)?,
      progress_bars,
      logger,
    })
  }

  pub fn download(&self, url: &str) -> Result<Option<Vec<u8>>> {
    let lowercase_url = url.to_lowercase();
    let agent = if lowercase_url.starts_with("https://") {
      &self.https_agent
    } else if lowercase_url.starts_with("http://") {
      &self.http_agent
    } else {
      bail!("Not implemented url scheme: {}", url);
    };
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

fn build_agent(kind: AgentKind, read_env_var: &impl Fn(&str) -> Option<String>) -> Result<ureq::Agent> {
  let mut agent = ureq::AgentBuilder::new();
  if let Some(proxy_url) = get_proxy_url(kind, read_env_var) {
    agent = agent.proxy(ureq::Proxy::new(proxy_url)?);
  }
  Ok(agent.build())
}

fn get_proxy_url(kind: AgentKind, read_env_var: &impl Fn(&str) -> Option<String>) -> Option<String> {
  match kind {
    AgentKind::Http => read_proxy_env_var("HTTP_PROXY", read_env_var),
    AgentKind::Https => read_proxy_env_var("HTTPS_PROXY", read_env_var),
  }
}

fn read_proxy_env_var(env_var_name: &str, read_env_var: &impl Fn(&str) -> Option<String>) -> Option<String> {
  read_env_var(&env_var_name.to_uppercase()).or_else(|| read_env_var(&env_var_name.to_lowercase()))
}

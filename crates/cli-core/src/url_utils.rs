use crate::logging::{ProgressBars, ProgressBarStyle};
use crate::types::ErrBox;
use std::io::Read;

pub fn download_url(url: &str, progress_bars: &Option<ProgressBars>, read_env_var: impl Fn(&str) -> Option<String>) -> Result<Vec<u8>, ErrBox> {
    let resp = match build_agent(url, read_env_var)?.get(url).call() {
        Ok(resp) => resp,
        Err(err) => return err!("Error downloading {}. Error: {:?}", url, err),
    };
    let total_size = {
        if resp.status() == 200 {
            resp.header("Content-Length")
                .and_then(|s| s.parse::<usize>().ok()).unwrap_or(0)
        } else {
            return err!("Error downloading {}. Status: {:?}", url, resp.status());
        }
    };
    let mut reader = resp.into_reader();
    match inner_download(url, &mut reader, total_size, progress_bars) {
        Ok(result) => Ok(result),
        Err(err) => err!("Error downloading {}. {}", url, err.to_string())
    }
}

fn inner_download(url: &str, reader: &mut impl Read, total_size: usize, progress_bars: &Option<ProgressBars>) -> Result<Vec<u8>, ErrBox> {
    let mut final_bytes = Vec::with_capacity(total_size);
    if let Some(progress_bars) = &progress_bars {
        let mut buf: [u8; 512] = [0; 512]; // ensure progress bars update often
        let message = format!("Downloading {}", url);
        let pb = progress_bars.add_progress(message, ProgressBarStyle::Download, total_size);
        loop {
            let bytes_read = reader.read(&mut buf)?;
            if bytes_read == 0 { break; }
            final_bytes.extend(&buf[..bytes_read]);
            pb.set_position(final_bytes.len());
        }
        pb.finish();
    } else {
        reader.read_to_end(&mut final_bytes)?;
    }
    Ok(final_bytes)
}

fn build_agent(url: &str, read_env_var: impl Fn(&str) -> Option<String>) -> Result<ureq::Agent, ErrBox> {
    let mut agent = ureq::AgentBuilder::new();
    if let Some(proxy_url) = get_proxy_url(url, read_env_var) {
        agent = agent.proxy(ureq::Proxy::new(proxy_url)?);
    }
    Ok(agent.build())
}

fn get_proxy_url(url: &str, read_env_var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let lower_url = url.to_lowercase();
    if lower_url.starts_with("https://") {
        read_proxy_env_var("HTTPS_PROXY", read_env_var)
    } else if lower_url.starts_with("http://") {
        read_proxy_env_var("HTTP_PROXY", read_env_var)
    } else {
        None
    }
}

fn read_proxy_env_var(env_var_name: &str, read_env_var: impl Fn(&str) -> Option<String>) -> Option<String> {
    read_env_var(&env_var_name.to_uppercase())
        .or_else(|| read_env_var(&env_var_name.to_lowercase()))
}
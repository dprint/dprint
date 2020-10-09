use crate::logging::{ProgressBars, ProgressBar, ProgressBarStyle};
use crate::types::ErrBox;
use reqwest::{Client, Response};

pub async fn download_url(url: &str, progress_bars: &Option<ProgressBars>) -> Result<Vec<u8>, ErrBox> {
    let client = Client::new();
    let resp = client.get(url).send().await?;
    if let Some(progress_bars) = &progress_bars {
        let message = format!("Downloading {}", url);
        let total_size = {
            if resp.status().is_success() {
                resp.content_length()
            } else {
                return err!("Error downloading: {}. Status: {:?}", url, resp.status());
            }
        }
        .unwrap_or(0) as usize;
        let pb = progress_bars.add_progress(message, ProgressBarStyle::Download, total_size);
        let result = inner_download(resp, total_size, &pb).await;
        pb.finish();
        result
    } else {
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec()) // todo: no into vector?
    }
}

async fn inner_download(resp: Response, total_size: usize, pb: &ProgressBar) -> Result<Vec<u8>, ErrBox> {
    let mut resp = resp;
    let mut final_bytes = Vec::with_capacity(total_size as usize);

    while let Some(chunk) = resp.chunk().await? {
        final_bytes.extend(chunk);
        pb.set_position(final_bytes.len());
    }

    Ok(final_bytes)
}
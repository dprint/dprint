use crate::{ProgressBars, ProgressBarStyle};
use crate::text_utils::get_middle_truncted_text;
use crate::types::ErrBox;
use reqwest::{Client, Response};
use bytes::Bytes;
use indicatif::ProgressBar;

pub async fn download_url(url: &str, progress_bars: &ProgressBars) -> Result<Bytes, ErrBox> {
    let client = Client::new();
    let resp = client.get(url).send().await?;
    let total_size = {
        if resp.status().is_success() {
            resp.content_length()
        } else {
            return err!("Error downloading: {}. Status: {:?}", url, resp.status());
        }
    }
    .unwrap_or(0);

    let message = get_middle_truncted_text("Downloading ", url);
    let pb = progress_bars
        .add_progress(&message, ProgressBarStyle::Download, total_size);
    let result = inner_download(resp, total_size, &pb).await;

    // ensure the progress bars are always cleared
    pb.finish_and_clear();
    progress_bars.finish_one().await?;

    result
}

async fn inner_download(resp: Response, total_size: u64, pb: &ProgressBar) -> Result<Bytes, ErrBox> {
    let mut resp = resp;
    let mut final_bytes = bytes::BytesMut::with_capacity(total_size as usize);

    while let Some(chunk) = resp.chunk().await? {
        final_bytes.extend_from_slice(&chunk);
        pb.set_position(final_bytes.len() as u64);
    }

    Ok(final_bytes.freeze())
}
use std::sync::{Arc, Mutex};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use crate::types::ErrBox;

struct ProgressState {
    progress: Arc<MultiProgress>,
    counter: usize,
    finish_handle: tokio::task::JoinHandle<()>,
}

struct InternalState {
    progress_state: Option<ProgressState>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ProgressBarStyle {
    Download,
    Action,
}

pub struct ProgressBars {
    state: Arc<Mutex<InternalState>>,
}

impl ProgressBars {
    pub fn new() -> Self {
        ProgressBars {
            state: Arc::new(Mutex::new(InternalState {
                progress_state: None,
            })),
        }
    }

    pub fn add_progress(&self, message: &str, style: ProgressBarStyle, total_size: u64) -> ProgressBar {
        let mut internal_state = self.state.lock().unwrap();
        let pb = if internal_state.progress_state.is_none() {
            let progress = Arc::new(MultiProgress::new());
            let pb = progress.add(ProgressBar::new(total_size));
            internal_state.progress_state = Some(ProgressState {
                progress: progress.clone(),
                counter: 1,
                finish_handle: tokio::task::spawn_blocking(move || {
                    progress.join_and_clear().unwrap();
                }),
            });
            pb
        } else {
            let internal_state = internal_state.progress_state.as_mut().unwrap();
            let pb = internal_state.progress.add(ProgressBar::new(total_size));
            internal_state.counter += 1;
            pb
        };

        pb.set_style(self.get_style(style));
        pb.set_message(message);

        pb
    }

    pub async fn finish_one(&self) -> Result<(), ErrBox> {
        let previous_state = {
            let mut internal_state = self.state.lock().unwrap();
            let mut progress_state = internal_state.progress_state.as_mut().expect("Cannot call finish() without a corresponding add_progress().");

            progress_state.counter -= 1;

            if progress_state.counter == 0 {
                internal_state.progress_state.take()
            } else {
                None
            }
        };

        if let Some(previous_state) = previous_state {
            previous_state.finish_handle.await?;
        }

        Ok(())
    }

    fn get_style(&self, style: ProgressBarStyle) -> ProgressStyle {
        match style {
            ProgressBarStyle::Download => {
                // https://github.com/mitsuhiko/indicatif/blob/main/examples/download.rs
                // https://github.com/mitsuhiko/indicatif/blob/main/examples/multi.rs
                ProgressStyle::default_bar()
                    .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                    .progress_chars("#>-")
            },
            ProgressBarStyle::Action => {
                ProgressStyle::default_bar()
                    .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}]")
                    .progress_chars("#>-")

            }
        }
    }
}

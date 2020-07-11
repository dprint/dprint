use std::sync::{Arc, Mutex};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

struct ProgressState {
    progress: Arc<MultiProgress>,
    counter: usize,
}

struct InternalState {
    progress_state: Option<ProgressState>,
    create_drawing_task: bool,
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
                create_drawing_task: true,
            })),
        }
    }

    pub fn add_progress(&self, message: &str, style: ProgressBarStyle, total_size: u64) -> ProgressBar {
        let mut internal_state = self.state.lock().unwrap();
        let create_drawing_task = internal_state.create_drawing_task;
        if internal_state.progress_state.is_none() {
            internal_state.progress_state = Some(ProgressState {
                progress: Arc::new(MultiProgress::new()),
                counter: 0,
            });
        }

        let mut progress_state = internal_state.progress_state.as_mut().unwrap();

        let pb = progress_state.progress.add(ProgressBar::new(total_size));
        pb.set_style(self.get_style(style));
        pb.set_message(message);

        progress_state.counter += 1;

        if create_drawing_task {
            let progress = progress_state.progress.clone();
            internal_state.create_drawing_task = false;
            let state = self.state.clone();
            tokio::task::spawn_blocking(move || {
                // Draw the progress on a dedicated task in order to prevent multiple threads
                // from drawing at the same time. Since one thread could stop all the progress
                // bars and another could immediately start before the join inside this task
                // completes. That would cause overlapping text.
                let mut progress = progress;
                loop {
                    progress.join_and_clear().unwrap();

                    // After exiting, if a new MultiProgress has been created then use it and
                    // continue drawing on this task.
                    let mut state = state.lock().unwrap();
                    if let Some(progress_state) = &state.progress_state.as_ref() {
                        progress = progress_state.progress.clone();
                    } else {
                        state.create_drawing_task = true;
                        break;
                    }
                }
            });
        }

        pb
    }

    pub fn finish_one(&self) {
        let mut state = self.state.lock().unwrap();
        let mut progress_state = state.progress_state.as_mut().expect("Cannot call finish() without a corresponding add_progress().");

        progress_state.counter -= 1;

        if progress_state.counter == 0 {
            state.progress_state.take();
        }
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

use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use crate::output_lock::OutputLock;
use crossterm::{style::{self, Colorize}, cursor, terminal, QueueableCommand};
use std::time::{Duration, SystemTime};
use std::io::{Stderr, stderr, Write};

mod log_action_with_progress;

pub use log_action_with_progress::*;

// Inspired by Indicatif, but this custom implementation allows for more control over
// what's going on under the hood and it works better with the multi-threading model
// going on in dprint.

#[derive(Clone, Copy, PartialEq)]
pub enum ProgressBarStyle {
    Download,
    Action,
}

#[derive(Clone)]
pub struct ProgressBar {
    id: usize,
    start_time: SystemTime,
    progress_bars: ProgressBars,
    message: String,
    size: usize,
    style: ProgressBarStyle,
    pos: Arc<RwLock<usize>>,
}

impl ProgressBar {
    pub fn set_position(&self, new_pos: usize) {
        let mut pos = self.pos.write();
        *pos = new_pos;
    }

    pub fn finish(&self) {
        self.progress_bars.finish_progress(self.id);
    }
}

#[derive(Clone)]
pub struct ProgressBars {
    output_lock: OutputLock,
    state: Arc<RwLock<InternalState>>,
    draw_state: Arc<Mutex<Option<DrawState>>>
}

struct InternalState {
    // this ensures only one draw thread is running
    drawer_id: usize,
    progress_bar_counter: usize,
    progress_bars: Vec<ProgressBar>,
}

struct DrawState {
    last_escaped_text: String,
}

impl ProgressBars {
    /// Checks if progress bars are supported
    pub fn are_supported() -> bool {
        get_terminal_width().is_some()
    }

    /// Creates a new ProgressBars or returns None when not supported.
    pub fn new(output_lock: &OutputLock) -> Option<Self> {
        if ProgressBars::are_supported() {
            Some(ProgressBars {
                output_lock: output_lock.clone(),
                state: Arc::new(RwLock::new(InternalState {
                    drawer_id: 0,
                    progress_bar_counter: 0,
                    progress_bars: Vec::new(),
                })),
                draw_state: Arc::new(Mutex::new(None)),
            })
        } else {
            None
        }
    }

    pub fn add_progress(&self, message: String, style: ProgressBarStyle, total_size: usize) -> ProgressBar {
        let mut internal_state = self.state.write();
        let id = internal_state.progress_bar_counter;
        let pb = ProgressBar {
            id,
            progress_bars: self.clone(),
            start_time: SystemTime::now(),
            message,
            size: total_size,
            style,
            pos: Arc::new(RwLock::new(0))
        };
        internal_state.progress_bars.push(pb.clone());
        internal_state.progress_bar_counter += 1;

        if internal_state.progress_bars.len() == 1 {
            self.start_draw_thread(&mut internal_state);
        }

        pb
    }

    fn finish_progress(&self, progress_bar_id: usize) {
        let mut internal_state = self.state.write();

        if let Some(index) = internal_state.progress_bars.iter().position(|p| p.id == progress_bar_id) {
            internal_state.progress_bars.remove(index);
        }

        if internal_state.progress_bars.is_empty() {
            let mut draw_state = self.draw_state.lock();

            let _g = self.output_lock.lock();
            let mut std_err = stderr();
            if let Some(draw_state) = draw_state.as_mut() {
                queue_clear_previous_draw(&mut std_err, &draw_state);
            }
            *draw_state = None;
            std_err.queue(cursor::Show).unwrap();
            let _ = std_err.flush();
        }
    }

    fn start_draw_thread(&self, internal_state: &mut InternalState) {
        let mut std_err = stderr();
        std_err.queue(cursor::Hide).unwrap();
        std_err.flush().unwrap();
        internal_state.drawer_id += 1;
        let drawer_id = internal_state.drawer_id;
        let output_lock = self.output_lock.clone();
        let draw_state = self.draw_state.clone();
        let internal_state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let mut std_err = stderr();
            loop {
                {
                    let internal_state = internal_state.read();
                    // exit if not the current draw thread or there are no more progress bars
                    if internal_state.drawer_id != drawer_id || internal_state.progress_bars.is_empty() {
                        break;
                    }

                    let terminal_width = get_terminal_width().unwrap();
                    let mut text = String::new();
                    for (i, progress_bar) in internal_state.progress_bars.iter().enumerate() {
                        if i > 0 { text.push_str("\n"); }
                        text.push_str(&progress_bar.message);
                        text.push_str("\n");
                        text.push_str(&get_progress_bar_text(
                            terminal_width,
                            *progress_bar.pos.read(),
                            progress_bar.size,
                            progress_bar.style,
                            progress_bar.start_time.elapsed().unwrap()
                        ));
                    }
                    let escaped_text = String::from_utf8(strip_ansi_escapes::strip(&text).unwrap()).unwrap();
                    let mut draw_state = draw_state.lock();

                    let _g = output_lock.lock();

                    if let Some(draw_state) = draw_state.as_mut() {
                        queue_clear_previous_draw(&mut std_err, &draw_state);
                        std_err.queue(style::Print(text)).unwrap();
                        std_err.flush().unwrap();
                        draw_state.last_escaped_text = escaped_text;
                    } else {
                        std_err.queue(style::Print(text.clone())).unwrap();
                        *draw_state = Some(DrawState {
                            last_escaped_text: escaped_text,
                        });
                    }

                    std_err.flush().unwrap();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
    }
}

fn queue_clear_previous_draw(std_err: &mut Stderr, draw_state: &DrawState) {
    let last_line_count = get_text_line_count(&draw_state.last_escaped_text, get_terminal_width().unwrap());
    if last_line_count > 0 {
        if last_line_count > 1 {
            std_err.queue(cursor::MoveUp(last_line_count - 1)).unwrap();
        }
        std_err.queue(cursor::MoveToColumn(0)).unwrap();
        std_err.queue(terminal::Clear(terminal::ClearType::FromCursorDown)).unwrap();
    }
}

fn get_text_line_count(text: &str, terminal_width: u16) -> u16 {
    let mut line_count: u16 = 0;
    let mut line_width: u16 = 0;
    for c in text.chars() {
        if c == '\n' {
            line_count += 1;
            line_width = 0;
        } else if line_width == terminal_width {
            line_width = 0;
            line_count += 1;
        } else {
            line_width += 1;
        }
    }
    line_count + 1
}

fn get_progress_bar_text(terminal_width: u16, pos: usize, total: usize, pb_style: ProgressBarStyle, duration: Duration) -> String {
    let bytes_text = if pb_style == ProgressBarStyle::Download {
        format!(" {}/{}", get_bytes_text(pos, total), get_bytes_text(total, total))
    } else {
        String::new()
    };

    let elapsed_text = get_elapsed_text(duration);
    let mut text = String::new();
    text.push_str(&elapsed_text);
    // get progress bar
    let percent = pos as f32 / total as f32;
    // don't include the bytes text in this because a string going from X.XXMB to XX.XXMB should not adjust the progress bar
    let total_bars = (std::cmp::min(50, terminal_width - 15) as usize) - elapsed_text.len() - 1 - 2;
    let completed_bars = (total_bars as f32 * percent).floor() as usize;
    text.push_str(" [");
    if completed_bars != total_bars {
        if completed_bars > 0 {
            text.push_str(&format!("{}", format!("{}{}", "#".repeat(completed_bars - 1), ">").cyan()))
        }
        text.push_str(&format!("{}", "-".repeat(total_bars - completed_bars).blue()))
    } else {
        text.push_str(&format!("{}", "#".repeat(completed_bars).cyan()))
    }
    text.push_str("]");

    // bytes text
    text.push_str(&bytes_text);

    text
}

fn get_bytes_text(byte_count: usize, total_bytes: usize) -> String {
    let bytes_to_kb = 1_000;
    let bytes_to_mb = 1_000_000;
    return if total_bytes < bytes_to_mb {
        get_in_format(byte_count, bytes_to_kb, "KB")
    } else {
        get_in_format(byte_count, bytes_to_mb, "MB")
    };

    fn get_in_format(byte_count: usize, conversion: usize, suffix: &str) -> String {
        let converted_value = byte_count / conversion;
        let decimal = (byte_count % conversion) * 100 / conversion;
        format!("{}.{:0>2}{}", converted_value, decimal, suffix)
    }
}

fn get_elapsed_text(elapsed: Duration) -> String {
    let elapsed_secs = elapsed.as_secs();
    let seconds = elapsed_secs % 60;
    let minutes = (elapsed_secs / 60) % 60;
    let hours = (elapsed_secs / 60) / 60;
    format!("[{:0>2}:{:0>2}:{:0>2}]", hours, minutes, seconds)
}

fn get_terminal_width() -> Option<u16> {
    match crossterm::terminal::size() {
        Ok((cols, _)) => Some(cols),
        Err(_) => None,
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;
    use super::*;

    #[test]
    fn it_should_get_bytes_text() {
        assert_eq!(get_bytes_text(9, 999), "0.00KB");
        assert_eq!(get_bytes_text(10, 999), "0.01KB");
        assert_eq!(get_bytes_text(100, 999), "0.10KB");
        assert_eq!(get_bytes_text(200, 999), "0.20KB");
        assert_eq!(get_bytes_text(520, 999), "0.52KB");
        assert_eq!(get_bytes_text(1000, 10_000), "1.00KB");
        assert_eq!(get_bytes_text(10_000, 10_000), "10.00KB");
        assert_eq!(get_bytes_text(999_999, 990_999), "999.99KB");
        assert_eq!(get_bytes_text(1_000_000, 1_000_000), "1.00MB");
        assert_eq!(get_bytes_text(9_524_102, 10_000_000), "9.52MB");
    }

    #[test]
    fn it_should_get_elapsed_text() {
        assert_eq!(get_elapsed_text(Duration::from_secs(1)), "[00:00:01]");
        assert_eq!(get_elapsed_text(Duration::from_secs(20)), "[00:00:20]");
        assert_eq!(get_elapsed_text(Duration::from_secs(59)), "[00:00:59]");
        assert_eq!(get_elapsed_text(Duration::from_secs(60)), "[00:01:00]");
        assert_eq!(get_elapsed_text(Duration::from_secs(60 * 5 + 23)), "[00:05:23]");
        assert_eq!(get_elapsed_text(Duration::from_secs(60 * 59 + 59)), "[00:59:59]");
        assert_eq!(get_elapsed_text(Duration::from_secs(60 * 60)), "[01:00:00]");
        assert_eq!(get_elapsed_text(Duration::from_secs(60 * 60 * 3 + 20 * 60 + 2)), "[03:20:02]");
        assert_eq!(get_elapsed_text(Duration::from_secs(60 * 60 * 99)), "[99:00:00]");
        assert_eq!(get_elapsed_text(Duration::from_secs(60 * 60 * 120)), "[120:00:00]");
    }
}

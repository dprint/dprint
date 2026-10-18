use deno_terminal::colors;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use crate::utils::get_terminal_size;
use crate::utils::is_terminal_interactive;

use super::Logger;
use super::LoggerRefreshItemKind;
use super::LoggerTextItem;

// Inspired by Indicatif, but this custom implementation allows for more control over
// what's going on under the hood and it works better with the multi-threading model
// going on in dprint.

/// Maximum width the bar section (elapsed text + bar) may take up.
const MAX_BAR_SECTION_WIDTH: usize = 50;
/// Width to assume when the terminal size can't be determined.
const DEFAULT_TERMINAL_WIDTH: u16 = 80;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProgressBarStyle {
  Download,
  Action,
}

pub struct ProgressBar {
  id: usize,
  logger: Arc<Logger>,
  state: Arc<Mutex<InternalState>>,
  pos: Arc<RwLock<usize>>,
}

impl Drop for ProgressBar {
  fn drop(&mut self) {
    self.finish();
  }
}

impl ProgressBar {
  pub fn set_position(&self, new_pos: usize) {
    let mut pos = self.pos.write();
    *pos = new_pos;
  }

  pub fn finish(&self) {
    let mut internal_state = self.state.lock();

    if let Some(index) = internal_state.progress_bars.iter().position(|p| p.id == self.id) {
      internal_state.progress_bars.remove(index);

      if internal_state.progress_bars.is_empty() {
        self.logger.remove_refresh_item(LoggerRefreshItemKind::ProgressBars);
        internal_state.drawer_id += 1;
      }
    }
  }
}

pub struct ProgressBars {
  logger: Arc<Logger>,
  state: Arc<Mutex<InternalState>>,
}

struct ProgressBarState {
  id: usize,
  start_time: SystemTime,
  message: String,
  size: usize,
  style: ProgressBarStyle,
  pos: Arc<RwLock<usize>>,
}

struct InternalState {
  // this ensures only one draw thread is running
  drawer_id: usize,
  progress_bar_counter: usize,
  progress_bars: Vec<ProgressBarState>,
}

impl ProgressBars {
  /// Checks if progress bars are supported
  pub fn are_supported() -> bool {
    is_terminal_interactive()
  }

  /// Creates a new ProgressBars or returns None when not supported.
  pub fn new(logger: &Arc<Logger>) -> Option<Self> {
    if ProgressBars::are_supported() {
      Some(ProgressBars {
        logger: logger.clone(),
        state: Arc::new(Mutex::new(InternalState {
          drawer_id: 0,
          progress_bar_counter: 0,
          progress_bars: Vec::new(),
        })),
      })
    } else {
      None
    }
  }

  pub fn add_progress(&self, message: String, style: ProgressBarStyle, total_size: usize) -> ProgressBar {
    let mut internal_state = self.state.lock();
    let id = internal_state.progress_bar_counter;
    let pos = Arc::new(RwLock::new(0));
    let pb_state = ProgressBarState {
      id,
      start_time: SystemTime::now(),
      message,
      size: total_size,
      style,
      pos: pos.clone(),
    };
    let pb = ProgressBar {
      id,
      logger: self.logger.clone(),
      state: self.state.clone(),
      pos,
    };
    internal_state.progress_bars.push(pb_state);
    internal_state.progress_bar_counter += 1;

    if internal_state.progress_bars.len() == 1 {
      self.start_draw_thread(&mut internal_state);
    }

    pb
  }

  fn start_draw_thread(&self, internal_state: &mut InternalState) {
    internal_state.drawer_id += 1;
    let drawer_id = internal_state.drawer_id;
    let internal_state = self.state.clone();
    let logger = self.logger.clone();
    dprint_core::async_runtime::spawn_blocking(move || {
      loop {
        {
          let internal_state = internal_state.lock();
          // exit if not the current draw thread or there are no more progress bars
          if internal_state.drawer_id != drawer_id || internal_state.progress_bars.is_empty() {
            break;
          }

          let terminal_width = get_terminal_size().map(|s| s.cols).unwrap_or(DEFAULT_TERMINAL_WIDTH);
          let mut text = String::new();
          for (i, progress_bar) in internal_state.progress_bars.iter().enumerate() {
            if i > 0 {
              text.push('\n');
            }
            text.push_str(&progress_bar.message);
            text.push('\n');
            text.push_str(&get_progress_bar_text(
              terminal_width,
              *progress_bar.pos.read(),
              progress_bar.size,
              progress_bar.style,
              progress_bar.start_time.elapsed().unwrap(),
            ));
          }

          logger.set_refresh_item(LoggerRefreshItemKind::ProgressBars, vec![LoggerTextItem::Text(text)]);
        }

        std::thread::sleep(Duration::from_millis(120));
      }
    });
  }
}

fn get_progress_bar_text(terminal_width: u16, pos: usize, total: usize, pb_style: ProgressBarStyle, duration: Duration) -> String {
  let total = std::cmp::max(pos, total); // increase the total when pos > total
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
  let total_bars = get_total_bars(terminal_width, elapsed_text.len());
  if total_bars == 0 {
    // the terminal is too narrow to draw a bar
    text.push_str(&bytes_text);
    return text;
  }
  let completed_bars = (total_bars as f32 * percent).floor() as usize;
  text.push_str(" [");
  if completed_bars != total_bars {
    if completed_bars > 0 {
      text.push_str(&format!("{}", colors::cyan(format!("{}{}", "#".repeat(completed_bars - 1), ">"))))
    }
    text.push_str(&format!("{}", colors::intense_blue("-".repeat(total_bars - completed_bars))))
  } else {
    text.push_str(&format!("{}", colors::cyan("#".repeat(completed_bars))))
  }
  text.push(']');

  // bytes text
  text.push_str(&bytes_text);

  text
}

fn get_total_bars(terminal_width: u16, elapsed_text_len: usize) -> usize {
  // reserve some space at the end for the bytes text
  let available_width = (terminal_width.saturating_sub(15) as usize).min(MAX_BAR_SECTION_WIDTH);
  // the bar is surrounded by ` [` and `]`, which the elapsed text is not
  available_width.saturating_sub(elapsed_text_len + 3)
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
  let minutes = elapsed_secs / 60;
  format!("[{:0>2}:{:0>2}]", minutes, seconds)
}

#[cfg(test)]
mod test {
  use super::*;
  use std::time::Duration;

  #[test]
  fn should_get_total_bars() {
    // "[00:00]".len() == 7
    assert_eq!(get_total_bars(80, 7), 40);
    assert_eq!(get_total_bars(65, 7), 40);
    assert_eq!(get_total_bars(64, 7), 39);
    assert_eq!(get_total_bars(40, 7), 15);
    assert_eq!(get_total_bars(26, 7), 1);
    assert_eq!(get_total_bars(25, 7), 0);
    // these used to underflow and panic with a capacity overflow (#1222)
    assert_eq!(get_total_bars(24, 7), 0);
    assert_eq!(get_total_bars(15, 7), 0);
    assert_eq!(get_total_bars(14, 7), 0);
    assert_eq!(get_total_bars(0, 7), 0);
    // "[5940:00]".len() == 9
    assert_eq!(get_total_bars(80, 9), 38);
    assert_eq!(get_total_bars(27, 9), 0);
  }

  #[test]
  fn should_not_draw_bar_when_terminal_too_narrow() {
    let text = get_progress_bar_text(20, 0, 10, ProgressBarStyle::Action, Duration::from_secs(1));
    assert_eq!(text, "[00:01]");
    let text = get_progress_bar_text(20, 5, 10, ProgressBarStyle::Download, Duration::from_secs(1));
    assert_eq!(text, "[00:01] 0.00KB/0.01KB");
  }

  #[test]
  fn should_get_progress_bar_text_for_any_terminal_width() {
    for terminal_width in 0..=200u16 {
      for style in [ProgressBarStyle::Action, ProgressBarStyle::Download] {
        for (pos, total) in [(0, 0), (0, 10), (1, 10), (5, 10), (10, 10), (20, 10)] {
          for duration in [Duration::from_secs(0), Duration::from_secs(60 * 60 * 99)] {
            get_progress_bar_text(terminal_width, pos, total, style, duration);
          }
        }
      }
    }
  }

  #[test]
  fn should_get_bytes_text() {
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
  fn should_get_elapsed_text() {
    assert_eq!(get_elapsed_text(Duration::from_secs(1)), "[00:01]");
    assert_eq!(get_elapsed_text(Duration::from_secs(20)), "[00:20]");
    assert_eq!(get_elapsed_text(Duration::from_secs(59)), "[00:59]");
    assert_eq!(get_elapsed_text(Duration::from_secs(60)), "[01:00]");
    assert_eq!(get_elapsed_text(Duration::from_secs(60 * 5 + 23)), "[05:23]");
    assert_eq!(get_elapsed_text(Duration::from_secs(60 * 59 + 59)), "[59:59]");
    assert_eq!(get_elapsed_text(Duration::from_secs(60 * 60)), "[60:00]");
    assert_eq!(get_elapsed_text(Duration::from_secs(60 * 60 * 3 + 20 * 60 + 2)), "[200:02]");
    assert_eq!(get_elapsed_text(Duration::from_secs(60 * 60 * 99)), "[5940:00]");
  }
}

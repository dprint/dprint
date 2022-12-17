use crossterm::style::Stylize;
use crossterm::tty::IsTty;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use crate::utils::get_terminal_size;

use super::Logger;
use super::LoggerRefreshItemKind;
use super::LoggerTextItem;

// Inspired by Indicatif, but this custom implementation allows for more control over
// what's going on under the hood and it works better with the multi-threading model
// going on in dprint.

#[derive(Clone, Copy, PartialEq, Eq)]
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
  logger: Logger,
  state: Arc<Mutex<InternalState>>,
}

struct InternalState {
  // this ensures only one draw thread is running
  drawer_id: usize,
  progress_bar_counter: usize,
  progress_bars: Vec<ProgressBar>,
}

impl ProgressBars {
  /// Checks if progress bars are supported
  pub fn are_supported() -> bool {
    std::io::stderr().is_tty() && get_terminal_size().is_some()
  }

  /// Creates a new ProgressBars or returns None when not supported.
  pub fn new(logger: &Logger) -> Option<Self> {
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
    let pb = ProgressBar {
      id,
      progress_bars: self.clone(),
      start_time: SystemTime::now(),
      message,
      size: total_size,
      style,
      pos: Arc::new(RwLock::new(0)),
    };
    internal_state.progress_bars.push(pb.clone());
    internal_state.progress_bar_counter += 1;

    if internal_state.progress_bars.len() == 1 {
      self.start_draw_thread(&mut internal_state);
    }

    pb
  }

  fn finish_progress(&self, progress_bar_id: usize) {
    let mut internal_state = self.state.lock();

    if let Some(index) = internal_state.progress_bars.iter().position(|p| p.id == progress_bar_id) {
      internal_state.progress_bars.remove(index);

      if internal_state.progress_bars.is_empty() {
        self.logger.remove_refresh_item(LoggerRefreshItemKind::ProgressBars);
        internal_state.drawer_id += 1;
      }
    }
  }

  fn start_draw_thread(&self, internal_state: &mut InternalState) {
    internal_state.drawer_id += 1;
    let drawer_id = internal_state.drawer_id;
    let internal_state = self.state.clone();
    let logger = self.logger.clone();
    tokio::task::spawn_blocking(move || {
      loop {
        {
          let internal_state = internal_state.lock();
          // exit if not the current draw thread or there are no more progress bars
          if internal_state.drawer_id != drawer_id || internal_state.progress_bars.is_empty() {
            break;
          }

          let terminal_width = get_terminal_size().unwrap().cols;
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
  text.push(']');

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
  let minutes = elapsed_secs / 60;
  format!("[{:0>2}:{:0>2}]", minutes, seconds)
}

#[cfg(test)]
mod test {
  use super::*;
  use std::time::Duration;

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

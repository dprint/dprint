use console_static_text::ConsoleSize;
use console_static_text::ConsoleStaticText;
use crossterm::QueueableCommand;
use crossterm::cursor;
use crossterm::style;
use parking_lot::Mutex;
use std::borrow::Cow;
use std::io::IsTerminal;
use std::io::Stderr;
use std::io::Stdout;
use std::io::Write;
use std::io::stderr;
use std::io::stdout;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;

use crate::utils::terminal::get_terminal_size;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LogLevel {
  Error = 0,
  Warn = 1,
  Info = 2,
  Debug = 3,
  Silent = 4,
}

impl LogLevel {
  #[inline]
  pub fn is_debug(&self) -> bool {
    use LogLevel::*;
    matches!(self, Debug)
  }

  #[inline]
  pub fn is_info(&self) -> bool {
    use LogLevel::*;
    matches!(self, Debug | Info)
  }

  #[inline]
  pub fn is_warn(&self) -> bool {
    use LogLevel::*;
    matches!(self, Debug | Info | Warn)
  }

  #[inline]
  pub fn is_error(&self) -> bool {
    use LogLevel::*;
    matches!(self, Debug | Info | Warn | Error)
  }
}

pub enum LoggerTextItem {
  Text(String),
  HangingText { text: String, indent: u16 },
}

impl LoggerTextItem {
  pub fn as_static_text_item(&self) -> console_static_text::TextItem<'_> {
    match self {
      LoggerTextItem::Text(text) => console_static_text::TextItem::Text(Cow::Borrowed(text.as_str())),
      LoggerTextItem::HangingText { text, indent } => console_static_text::TextItem::HangingText {
        text: Cow::Borrowed(text.as_str()),
        indent: *indent,
      },
    }
  }
}

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub(crate) enum LoggerRefreshItemKind {
  // numbered by display order
  ProgressBars = 0,
  Selection = 1,
}

struct LoggerRefreshItem {
  kind: LoggerRefreshItemKind,
  text_items: Vec<LoggerTextItem>,
}

#[derive(Clone)]
pub struct LoggerOptions {
  pub initial_context_name: String,
  /// Whether stdout will be read by a program.
  pub is_stdout_machine_readable: bool,
  pub log_level: LogLevel,
}

pub struct Logger {
  output_lock: Mutex<LoggerState>,
  is_stdout_machine_readable: bool,
  /// The `LogLevel` as a number so it can be changed while logging (see
  /// `Logger::set_log_level`).
  log_level: AtomicU8,
}

struct LoggerState {
  last_context_name: String,
  std_out: Stdout,
  std_err: Stderr,
  refresh_items: Vec<LoggerRefreshItem>,
  static_text: ConsoleStaticText,
}

impl Logger {
  pub fn new(options: &LoggerOptions) -> Self {
    Logger {
      output_lock: Mutex::new(LoggerState {
        last_context_name: options.initial_context_name.clone(),
        std_out: stdout(),
        std_err: stderr(),
        refresh_items: Vec::new(),
        static_text: ConsoleStaticText::new(|| {
          let size = get_terminal_size();
          ConsoleSize {
            cols: size.map(|s| s.cols),
            rows: size.map(|s| s.rows),
          }
        }),
      }),
      is_stdout_machine_readable: options.is_stdout_machine_readable,
      log_level: AtomicU8::new(options.log_level as u8),
    }
  }

  #[inline]
  pub fn log_level(&self) -> LogLevel {
    match self.log_level.load(Ordering::Relaxed) {
      0 => LogLevel::Error,
      1 => LogLevel::Warn,
      2 => LogLevel::Info,
      3 => LogLevel::Debug,
      _ => LogLevel::Silent,
    }
  }

  /// Changes the log level (ex. when an editor's setting for it changes).
  pub fn set_log_level(&self, log_level: LogLevel) {
    self.log_level.store(log_level as u8, Ordering::Relaxed);
  }

  pub fn log(&self, text: &str, context_name: &str) {
    if self.is_stdout_machine_readable {
      return;
    }
    let mut state = self.output_lock.lock();
    self.inner_log(&mut state, true, text, context_name);
  }

  pub fn log_machine_readable(&self, text_bytes: &[u8]) {
    let mut state = self.output_lock.lock();
    // This should clear stderr before outputting stdout,
    // but it's never used in a situation where it matters
    // so for simplicitly that hasn't been implemented here
    state.std_out.write_all(text_bytes).unwrap();
  }

  /// Don't ever call this directly. It's only used by the macro, which
  /// is why it has this name to discourage its accidental use.
  pub fn __log_stderr__(&self, text: &str) {
    self.log_stderr_with_context(text, "dprint");
  }

  pub fn log_stderr_with_context(&self, text: &str, context_name: &str) {
    let mut state = self.output_lock.lock();
    self.inner_log(&mut state, false, text, context_name);
  }

  pub fn log_text_items(&self, text_items: &[LoggerTextItem], context_name: &str) {
    let terminal_width = get_terminal_size().map(|s| s.cols);
    let text = render_text_items_with_width(text_items, terminal_width);
    self.log(&text, context_name);
  }

  fn inner_log(&self, state: &mut LoggerState, is_std_out: bool, text: &str, context_name: &str) {
    let mut stderr_text = String::new();

    // only get the terminal size if there are refresh items
    let terminal_size = if state.refresh_items.is_empty() {
      None
    } else {
      Some(state.static_text.console_size())
    };
    if let Some(terminal_size) = terminal_size
      && let Some(text) = state.static_text.render_clear_with_size(terminal_size)
    {
      stderr_text = text;
    }

    let mut output_text = String::new();
    if state.last_context_name != context_name {
      // don't output this if stdout is machine readable
      if !is_std_out || !self.is_stdout_machine_readable {
        output_text.push_str(&format!("[{}]\n", context_name));
      }
      state.last_context_name = context_name.to_string();
    }

    output_text.push_str(text);

    // only add a newline if the logged text does not end with one
    if !output_text.ends_with('\n') {
      output_text.push('\n');
    }
    let is_raw_terminal = if is_std_out {
      is_raw_mode_terminal(&state.std_out)
    } else {
      is_raw_mode_terminal(&state.std_err)
    };
    if is_raw_terminal {
      output_text = output_text.replace("\r\n", "\n").replace('\n', "\r\n");
    }

    if is_std_out {
      if !output_text.is_empty() {
        // render the stderr clear first
        if !stderr_text.is_empty() {
          write!(state.std_err, "{}", stderr_text).unwrap();
          state.std_err.flush().unwrap();
          stderr_text.clear();
        }
        // now output stdout
        write!(state.std_out, "{}", output_text).unwrap();
        state.std_out.flush().unwrap();
      }
    } else {
      stderr_text.push_str(&output_text);
    }

    // finally render stderr
    if let Some(terminal_size) = terminal_size
      && let Some(text) = self.render_draw_items(state, terminal_size)
    {
      stderr_text.push_str(&text);
    }

    if !stderr_text.is_empty() {
      write!(state.std_err, "{}", stderr_text).unwrap();
      state.std_err.flush().unwrap();
    }
  }

  pub(crate) fn set_refresh_item(&self, kind: LoggerRefreshItemKind, text_items: Vec<LoggerTextItem>) {
    self.with_update_refresh_items(move |refresh_items| match refresh_items.binary_search_by(|i| i.kind.cmp(&kind)) {
      Ok(pos) => {
        let refresh_item = refresh_items.get_mut(pos).unwrap();
        refresh_item.text_items = text_items;
      }
      Err(pos) => {
        let refresh_item = LoggerRefreshItem { kind, text_items };
        refresh_items.insert(pos, refresh_item);
      }
    });
  }

  pub(crate) fn remove_refresh_item(&self, kind: LoggerRefreshItemKind) {
    self.with_update_refresh_items(move |refresh_items| {
      if let Ok(pos) = refresh_items.binary_search_by(|i| i.kind.cmp(&kind)) {
        refresh_items.remove(pos);
      } else {
        // already removed
      }
    });
  }

  fn with_update_refresh_items(&self, update_refresh_items: impl FnOnce(&mut Vec<LoggerRefreshItem>)) {
    let mut state = self.output_lock.lock();

    // hide the cursor if showing a refresh item for the first time
    if state.refresh_items.is_empty() {
      state.std_err.queue(cursor::Hide).unwrap();
    }

    update_refresh_items(&mut state.refresh_items);

    let size = state.static_text.console_size();
    if let Some(text) = self.render_draw_items(&mut state, size) {
      state.std_err.queue(style::Print(text)).unwrap();
    }

    // show the cursor if no longer showing a refresh item
    if state.refresh_items.is_empty() {
      state.std_err.queue(cursor::Show).unwrap();
    }
    state.std_err.flush().unwrap();
  }

  fn render_draw_items(&self, state: &mut LoggerState, size: ConsoleSize) -> Option<String> {
    let text_items = state.refresh_items.iter().flat_map(|item| item.text_items.iter());
    let text_items = text_items.map(|i| i.as_static_text_item()).collect::<Vec<_>>();
    state.static_text.render_items_with_size(text_items.iter(), size)
  }
}

/// Whether the stream is a terminal in raw mode, which happens when something
/// is logged while a key is read (ex. a prompt waiting on a download). A
/// newline doesn't go back to the start of the line there on unix, so it
/// needs a carriage return.
fn is_raw_mode_terminal(stream: &impl IsTerminal) -> bool {
  crossterm::terminal::is_raw_mode_enabled().unwrap_or(false) && stream.is_terminal()
}

/// Renders the text items with the specified width.
pub fn render_text_items_with_width(text_items: &[LoggerTextItem], terminal_width: Option<u16>) -> String {
  let mut static_text = ConsoleStaticText::new(move || ConsoleSize {
    cols: terminal_width,
    rows: None,
  });
  static_text.keep_cursor_zero_column(false);
  let text_items = text_items.iter().map(|i| i.as_static_text_item()).collect::<Vec<_>>();
  static_text
    .render_items_with_size(
      text_items.iter(),
      ConsoleSize {
        cols: terminal_width,
        rows: None,
      },
    )
    .unwrap_or_default()
}

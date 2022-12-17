use console_static_text::ConsoleSize;
use console_static_text::ConsoleStaticText;
use crossterm::cursor;
use crossterm::style;
use crossterm::QueueableCommand;
use parking_lot::Mutex;
use std::io::stderr;
use std::io::stdout;
use std::io::Stderr;
use std::io::Stdout;
use std::io::Write;
use std::sync::Arc;

use crate::utils::terminal::get_terminal_size;

pub enum LoggerTextItem {
  Text(String),
  HangingText { text: String, indent: u16 },
}

impl LoggerTextItem {
  pub fn as_static_text_item(&self) -> console_static_text::TextItem {
    match self {
      LoggerTextItem::Text(text) => console_static_text::TextItem::Text(text.as_str()),
      LoggerTextItem::HangingText { text, indent } => console_static_text::TextItem::HangingText {
        text: text.as_str(),
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
  pub is_verbose: bool,
}

#[derive(Clone)]
pub struct Logger {
  output_lock: Arc<Mutex<LoggerState>>,
  is_stdout_machine_readable: bool,
  is_verbose: bool,
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
      output_lock: Arc::new(Mutex::new(LoggerState {
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
      })),
      is_stdout_machine_readable: options.is_stdout_machine_readable,
      is_verbose: options.is_verbose,
    }
  }

  #[inline]
  pub fn is_verbose(&self) -> bool {
    self.is_verbose
  }

  pub fn log(&self, text: &str, context_name: &str) {
    if self.is_stdout_machine_readable {
      return;
    }
    let mut state = self.output_lock.lock();
    self.inner_log(&mut state, true, text, context_name);
  }

  pub fn log_machine_readable(&self, text: &str) {
    let mut state = self.output_lock.lock();
    let last_context_name = state.last_context_name.clone(); // not really used here
    self.inner_log(&mut state, true, text, &last_context_name);
  }

  pub fn log_stderr(&self, text: &str) {
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
    let mut stdout_text = String::new();
    let terminal_size = state.static_text.console_size();
    if let Some(text) = state.static_text.render_clear_with_size(terminal_size) {
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

    if is_std_out {
      stdout_text.push_str(&output_text);
    } else {
      stderr_text.push_str(&output_text);
    }

    if let Some(text) = self.render_draw_items(state, terminal_size) {
      stderr_text.push_str(&text);
    }

    if !stdout_text.is_empty() {
      write!(state.std_out, "{}", stdout_text).unwrap();
      state.std_out.flush().unwrap();
    }
    if !stderr_text.is_empty() {
      write!(state.std_err, "{}", stderr_text).unwrap();
      state.std_err.flush().unwrap();
    }
  }

  pub(crate) fn set_refresh_item(&self, kind: LoggerRefreshItemKind, text_items: Vec<LoggerTextItem>) {
    self.with_update_refresh_items(move |refresh_items| match refresh_items.binary_search_by(|i| i.kind.cmp(&kind)) {
      Ok(pos) => {
        let mut refresh_item = refresh_items.get_mut(pos).unwrap();
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
    let text_items = text_items.map(|i| i.as_static_text_item());
    state.static_text.render_items_with_size(text_items, size)
  }
}

/// Renders the text items with the specified width.
pub fn render_text_items_with_width(text_items: &[LoggerTextItem], terminal_width: Option<u16>) -> String {
  let mut static_text = ConsoleStaticText::new(move || ConsoleSize {
    cols: terminal_width,
    rows: None,
  });
  static_text.keep_cursor_zero_column(false);
  let static_text_items = text_items.iter().map(|i| i.as_static_text_item());
  static_text
    .render_items_with_size(
      static_text_items,
      ConsoleSize {
        cols: terminal_width,
        rows: None,
      },
    )
    .unwrap_or_default()
}

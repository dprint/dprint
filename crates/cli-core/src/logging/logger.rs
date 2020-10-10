use std::io::{Stdout, Stderr, stderr, stdout, Write};
use std::sync::Arc;
use crossterm::{style, cursor, terminal, QueueableCommand};
use parking_lot::Mutex;

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub(crate) enum LoggerRefreshItemKind {
    // numbered by display order
    ProgressBars = 0,
    Selection = 1,
}

struct LoggerRefreshItem {
    kind: LoggerRefreshItemKind,
    text: String,
    escaped_text: String,
}

#[derive(Clone)]
pub struct Logger {
    output_lock: Arc<Mutex<LoggerState>>,
}

struct LoggerState {
    is_silent: bool,
    last_context_name: String,
    std_out: Stdout,
    std_err: Stderr,
    refresh_items: Vec<LoggerRefreshItem>,
}

impl Logger {
    pub fn new(initial_context_name: &str, is_silent: bool) -> Self {
        Logger {
            output_lock: Arc::new(Mutex::new(LoggerState {
                is_silent,
                last_context_name: initial_context_name.to_string(),
                std_out: stdout(),
                std_err: stderr(),
                refresh_items: Vec::new(),
            })),
        }
    }

    pub fn log(&self, text: &str, context_name: &str) {
        let mut state = self.output_lock.lock();
        if state.is_silent { return; }
        self.inner_log(&mut state, true, text, context_name);
    }

    pub fn log_bypass_silent(&self, text: &str, context_name: &str) {
        let mut state = self.output_lock.lock();
        self.inner_log(&mut state, true, text, context_name);
    }

    pub fn log_err(&self, text: &str, context_name: &str) {
        let mut state = self.output_lock.lock();
        self.inner_log(&mut state, false, text, context_name);
    }

    fn inner_log(&self, state: &mut LoggerState, is_std_out: bool, text: &str, context_name: &str) {
        if !state.refresh_items.is_empty() {
            self.inner_queue_clear_previous_draws(state);
        }

        let mut output_text = String::new();
        if state.last_context_name != context_name {
            output_text.push_str(&format!("[{}]\n", context_name));
            state.last_context_name = context_name.to_string();
        }
        output_text.push_str(text);
        output_text.push_str("\n");

        if is_std_out {
            state.std_out.queue(style::Print(output_text)).unwrap();
        } else {
            state.std_err.queue(style::Print(output_text)).unwrap();
        }

        if !state.refresh_items.is_empty() {
            self.inner_queue_draw_items(state);
        }

        if is_std_out {
            state.std_out.flush().unwrap();
            if !state.refresh_items.is_empty() {
                state.std_err.flush().unwrap();
            }
        } else {
            state.std_err.flush().unwrap();
        }
    }

    pub(crate) fn set_refresh_item(&self, kind: LoggerRefreshItemKind, text: String) {
        self.with_update_refresh_items(move |refresh_items| {
            let escaped_text = String::from_utf8(strip_ansi_escapes::strip(&text).unwrap()).unwrap();
            match refresh_items.binary_search_by(|i| i.kind.cmp(&kind)) {
                Ok(pos) => {
                    let mut refresh_item = refresh_items.get_mut(pos).unwrap();
                    refresh_item.escaped_text = escaped_text;
                    refresh_item.text = text;
                },
                Err(pos) => {
                    let refresh_item = LoggerRefreshItem {
                        kind,
                        text,
                        escaped_text,
                    };
                    refresh_items.insert(pos, refresh_item);
                }
            }
        });
    }

    pub(crate) fn remove_refresh_item(&self, kind: LoggerRefreshItemKind) {
        self.with_update_refresh_items(move |refresh_items| {
            match refresh_items.binary_search_by(|i| i.kind.cmp(&kind)) {
                Ok(pos) => {
                    refresh_items.remove(pos);
                },
                _ => {}, // already removed
            }
        });
    }

    fn with_update_refresh_items(&self, update_refresh_items: impl FnOnce(&mut Vec<LoggerRefreshItem>)) {
        let mut state = self.output_lock.lock();

        // hide the cursor if showing a refresh item for the first time
        if state.refresh_items.is_empty() {
            state.std_err.queue(cursor::Hide).unwrap();
        }

        self.inner_queue_clear_previous_draws(&mut state);

        update_refresh_items(&mut state.refresh_items);

        self.inner_queue_draw_items(&mut state);

        // show the cursor if no longer showing a refresh item
        if state.refresh_items.is_empty() {
            state.std_err.queue(cursor::Show).unwrap();
        }
        state.std_err.flush().unwrap();
    }

    fn inner_queue_clear_previous_draws(&self, state: &mut LoggerState) {
        let terminal_width = crate::terminal::get_terminal_width().unwrap();
        let mut last_line_count = 0;
        for item in state.refresh_items.iter() {
            last_line_count += get_text_line_count(&item.escaped_text, terminal_width)
        }
        if last_line_count > 0 {
            if last_line_count > 1 {
                state.std_err.queue(cursor::MoveUp(last_line_count - 1)).unwrap();
            }
            state.std_err.queue(cursor::MoveToColumn(0)).unwrap();
            state.std_err.queue(terminal::Clear(terminal::ClearType::FromCursorDown)).unwrap();
        }
    }

    fn inner_queue_draw_items(&self, state: &mut LoggerState) {
        for (i, item) in state.refresh_items.iter().enumerate() {
            if i > 0 { state.std_err.queue(style::Print("\n")).unwrap(); }
            state.std_err.queue(style::Print(&item.text)).unwrap();
        }
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

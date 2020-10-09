use std::io::{Stdout, Stderr, stderr, stdout, Write};
use std::sync::Arc;
use crossterm::{style, cursor, terminal, QueueableCommand};
use parking_lot::Mutex;

#[derive(Clone)]
pub struct Logger {
    output_lock: Arc<Mutex<LoggerState>>,
}

struct LoggerState {
    is_silent: bool,
    last_context_name: String,
    std_out: Stdout,
    std_err: Stderr,
    has_progress_bars: bool,
    last_escaped_progress_bar_text: Option<String>,
}

impl Logger {
    pub fn new(initial_context_name: &str, is_silent: bool) -> Self {
        Logger {
            output_lock: Arc::new(Mutex::new(LoggerState {
                is_silent,
                last_context_name: initial_context_name.to_string(),
                std_out: stdout(),
                std_err: stderr(),
                has_progress_bars: false,
                last_escaped_progress_bar_text: None,
            })),
        }
    }

    pub fn log(&self, text: &str, context_name: &str) {
        let mut state = self.output_lock.lock();
        if state.is_silent { return; }
        self.inner_log(&mut state, text, context_name);
    }

    pub fn log_bypass_silent(&self, text: &str, context_name: &str) {
        let mut state = self.output_lock.lock();
        self.inner_log(&mut state, text, context_name);
    }

    fn inner_log(&self, state: &mut LoggerState, text: &str, context_name: &str) {
        if state.has_progress_bars {
            // don't bother redrawing... it will draw back on its own
            self.inner_queue_clear_progress_bars(state);
            let _ = state.std_err.flush();
        }
        if state.last_context_name != context_name {
            writeln!(&mut state.std_out, "[{}]", context_name).unwrap();
            state.last_context_name = context_name.to_string();
        }
        writeln!(&mut state.std_out, "{}", text).unwrap();
    }

    pub fn log_err(&self, text: &str, context_name: &str) {
        let mut state = self.output_lock.lock();
        if state.is_silent { return; }
        if state.has_progress_bars {
            self.inner_queue_clear_progress_bars(&mut state);
            let _ = state.std_err.flush();
        }
        if state.last_context_name != context_name {
            writeln!(&mut state.std_err, "[{}]", context_name).unwrap();
            state.last_context_name = context_name.to_string();
        }
        writeln!(&mut state.std_err, "{}", text).unwrap();
    }

    pub fn draw_progress_bars(&self, text: String) {
        let escaped_text = String::from_utf8(strip_ansi_escapes::strip(&text).unwrap()).unwrap();
        let mut state = self.output_lock.lock();
        if state.is_silent { return; }

        if !state.has_progress_bars {
            state.std_err.queue(cursor::Hide).unwrap();
        }

        state.has_progress_bars = true;

        self.inner_queue_clear_progress_bars(&mut state);

        state.std_err.queue(style::Print(text)).unwrap();
        state.std_err.flush().unwrap();

        state.std_err.flush().unwrap();
        state.last_escaped_progress_bar_text = Some(escaped_text);
    }

    pub fn clear_progress_bars(&self) {
        let mut state = self.output_lock.lock();
        if state.is_silent { return; }

        self.inner_queue_clear_progress_bars(&mut state);
        state.std_err.queue(cursor::Show).unwrap();

        let _ = state.std_err.flush();
        state.has_progress_bars = false;
    }

    fn inner_queue_clear_progress_bars(&self, state: &mut LoggerState) {
        if let Some(last_escaped_progress_bar_text) = state.last_escaped_progress_bar_text.take() {
            queue_clear_previous_draw(&mut state.std_err, &last_escaped_progress_bar_text);
        }
    }
}

fn queue_clear_previous_draw(std_err: &mut Stderr, last_escaped_text: &str) {
    let terminal_width = crate::terminal::get_terminal_width().unwrap();
    let last_line_count = get_text_line_count(&last_escaped_text, terminal_width);
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

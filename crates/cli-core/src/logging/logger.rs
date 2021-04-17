use std::io::{Stdout, Stderr, stderr, stdout, Write};
use std::sync::Arc;
use crossterm::{style, cursor, terminal, QueueableCommand};
use parking_lot::Mutex;

pub enum LoggerTextItem {
    Text(String),
    HangingText { text: String, indent: u16, }
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
pub struct Logger {
    output_lock: Arc<Mutex<LoggerState>>,
}

struct LoggerState {
    is_silent: bool,
    last_context_name: String,
    std_out: Stdout,
    std_err: Stderr,
    refresh_items: Vec<LoggerRefreshItem>,
    last_terminal_size: Option<(u16, u16)>,
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
                last_terminal_size: None,
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

    pub fn log_text_items(&self, text_items: &Vec<LoggerTextItem>, context_name: &str, terminal_width: Option<u16>) {
        let text = render_text_items_with_width(text_items, terminal_width);
        self.log(&text, context_name);
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

        // only add a newline if the logged text does not end with one
        if !output_text.ends_with("\n") {
            output_text.push_str("\n");
        }

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

    pub(crate) fn set_refresh_item(&self, kind: LoggerRefreshItemKind, text_items: Vec<LoggerTextItem>) {
        self.with_update_refresh_items(move |refresh_items| {
            match refresh_items.binary_search_by(|i| i.kind.cmp(&kind)) {
                Ok(pos) => {
                    let mut refresh_item = refresh_items.get_mut(pos).unwrap();
                    refresh_item.text_items = text_items;
                },
                Err(pos) => {
                    let refresh_item = LoggerRefreshItem {
                        kind,
                        text_items,
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
        let text_items = state.refresh_items.iter().map(|item| item.text_items.iter()).flatten();
        let rendered_text = render_text_items_truncated_to_height(text_items, state.last_terminal_size);
        let last_line_count = get_text_line_count(&rendered_text, terminal_width);

        if last_line_count > 0 {
            if last_line_count > 1 {
                state.std_err.queue(cursor::MoveUp(last_line_count - 1)).unwrap();
            }
            state.std_err.queue(cursor::MoveToColumn(0)).unwrap();
            state.std_err.queue(terminal::Clear(terminal::ClearType::FromCursorDown)).unwrap();
        }

        state.std_err.queue(cursor::MoveToColumn(0)).unwrap();
    }

    fn inner_queue_draw_items(&self, state: &mut LoggerState) {
        let terminal_size = crate::terminal::get_terminal_size();
        let text_items = state.refresh_items.iter().map(|item| item.text_items.iter()).flatten();
        let rendered_text = render_text_items_truncated_to_height(text_items, terminal_size);
        state.std_err.queue(style::Print(&rendered_text)).unwrap();
        state.std_err.queue(cursor::MoveToColumn(0)).unwrap();
        state.last_terminal_size = terminal_size;
    }
}

/// Renders the text items with the specified width.
pub fn render_text_items_with_width(text_items: &Vec<LoggerTextItem>, terminal_width: Option<u16>) -> String {
    render_text_items_to_lines(text_items.iter(), terminal_width).join("\n")
}

fn render_text_items_truncated_to_height<'a>(text_items: impl Iterator<Item=&'a LoggerTextItem>, terminal_size: Option<(u16, u16)>) -> String {
    let lines = render_text_items_to_lines(text_items, terminal_size.map(|(width, _)| width));
    if let Some(height) = terminal_size.map(|(_, height)| height) {
        let max_height = height as usize;
        if lines.len() > max_height  {
            return lines[lines.len() - max_height..].join("\n");
        }
    }
    lines.join("\n")
}

fn render_text_items_to_lines<'a>(text_items: impl Iterator<Item=&'a LoggerTextItem>, terminal_width: Option<u16>) -> Vec<String> {
    let mut result = Vec::new();
    for (_, item) in text_items.enumerate() {
        match item {
            LoggerTextItem::Text(text) =>
                result.extend(render_text_to_lines(text, 0, terminal_width)),
            LoggerTextItem::HangingText { text, indent } => {
                result.extend(render_text_to_lines(text, *indent, terminal_width));
            },
        }
    }
    result
}

fn render_text_to_lines(text: &str, hanging_indent: u16, terminal_width: Option<u16>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(terminal_width) = terminal_width {
        let mut current_line = String::new();
        let mut line_width: u16 = 0;
        let mut current_whitespace = String::new();
        for token in tokenize_words(&text) {
            match token {
                WordToken::Word((word, word_width)) => {
                    let is_word_longer_than_line = hanging_indent + word_width > terminal_width;
                    if is_word_longer_than_line {
                        // break it up onto multiple lines with indentation
                        if !current_whitespace.is_empty() {
                            if line_width < terminal_width {
                                current_line.push_str(&current_whitespace);
                            }
                            current_whitespace = String::new();
                        }
                        for c in word.chars() {
                            if line_width == terminal_width {
                                lines.push(current_line);
                                current_line = String::new();
                                current_line.push_str(&" ".repeat(hanging_indent as usize));
                                line_width = hanging_indent;
                            }
                            current_line.push(c);
                            line_width += 1;
                        }
                    }
                    else {
                        if line_width + word_width > terminal_width {
                            lines.push(current_line);
                            current_line = String::new();
                            current_line.push_str(&" ".repeat(hanging_indent as usize));
                            line_width = hanging_indent;
                            current_whitespace = String::new();
                        }
                        if !current_whitespace.is_empty() {
                            current_line.push_str(&current_whitespace);
                            current_whitespace = String::new();
                        }
                        current_line.push_str(&word);
                        line_width += word_width;
                    }
                }
                WordToken::WhiteSpace(space_char) => {
                    current_whitespace.push(space_char);
                    line_width += 1;
                }
                WordToken::NewLine => {
                    lines.push(current_line);
                    current_line = String::new();
                    line_width = 0;
                }
            }
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }
    } else {
        for line in text.lines() {
            lines.push(line.to_string());
        }
    }
    lines
}

enum WordToken<'a> {
    Word((&'a str, u16)),
    WhiteSpace(char),
    NewLine,
}

fn tokenize_words<'a>(text: &'a str) -> Vec<WordToken<'a>> {
    // todo: how to write an iterator version?
    let mut start_index = 0;
    let mut tokens = Vec::new();
    let mut word_width = 0;
    for (index, c) in text.char_indices() {
        if c.is_whitespace() || c == '\n' {
            if word_width > 0 {
                tokens.push(WordToken::Word((&text[start_index..index], word_width)));
                word_width = 0;
            }

            if c == '\n' {
                tokens.push(WordToken::NewLine);
            } else {
                tokens.push(WordToken::WhiteSpace(c));
            }

            start_index = index + c.len_utf8(); // start at next char
        } else {
            if !c.is_ascii_control() {
                word_width += 1;
            }
        }
    }
    if word_width > 0 {
        tokens.push(WordToken::Word((&text[start_index..text.len()], word_width)));
    }
    tokens
}

fn get_text_line_count(text: &str, terminal_width: u16) -> u16 {
    let mut line_count: u16 = 0;
    let mut line_width: u16 = 0;
    for c in text.chars() {
        if c.is_ascii_control() && !c.is_ascii_whitespace() {
            // ignore
        } else if c == '\n' {
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

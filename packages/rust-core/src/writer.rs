#[derive(Clone)]
pub struct WriterState {
    pub current_line_column: u32,
    pub current_line_number: u32,
    pub last_line_indent_level: u16,
    pub indent_level: u16,
    pub expect_newline_next: bool,
    pub items: Vec<String>,
    pub ignore_indent_count: u8,
}

pub struct WriterOptions {
    pub indent_width: u8,
    pub use_tabs: bool,
    pub newline_kind: &'static str,
}

pub struct Writer {
    state: WriterState,
    single_indentation_text: String,
    indent_width: u8,
    newline_kind: String,
}

impl Writer {
    pub fn new(options: WriterOptions) -> Writer {
        let single_indentation_text = if options.use_tabs {
            String::from("\t")
        } else {
            String::from(" ").repeat(options.indent_width as usize)
        };

        Writer {
            indent_width: options.indent_width,
            single_indentation_text,
            newline_kind: String::from(options.newline_kind),
            state: WriterState {
                current_line_column: 0,
                current_line_number: 0,
                last_line_indent_level: 0,
                indent_level: 0,
                expect_newline_next: false,
                items: Vec::new(),
                ignore_indent_count: 0,
            },
        }
    }

    pub fn get_state(&self) -> WriterState {
        self.state.clone()
    }

    pub fn set_state(&mut self, state: WriterState) {
        self.state = state;
    }

    pub fn start_indent(&mut self) {
        self.state.indent_level += 1;
    }

    pub fn finish_indent(&mut self) {
        if self.state.indent_level == 0 {
            panic!("For some reason finish_indent was called without a corresponding start_indent.");
        }

        self.state.indent_level -= 1;
    }

    pub fn start_ignoring_indent(&mut self) {
        self.state.ignore_indent_count += 1;
    }

    pub fn finish_ignoring_indent(&mut self) {
        self.state.ignore_indent_count -= 1;
    }

    pub fn mark_expect_new_line(&mut self) {
        self.state.expect_newline_next = true;
    }

    pub fn get_line_start_indent_level(&self) -> u16 {
        self.state.last_line_indent_level
    }

    pub fn get_indentation_level(&self) -> u16 {
        self.state.indent_level
    }

    pub fn get_line_start_column_number(&self) -> u32 {
        // every char should be 1 byte so should be ok to use len() here
        (self.single_indentation_text.len() as u32) * (self.state.last_line_indent_level as u32)
    }

    pub fn get_line_column(&self) -> u32 {
        if self.state.current_line_column == 0 {
            (self.indent_width as u32) * (self.state.indent_level as u32)
        } else {
            self.state.current_line_column
        }
    }

    pub fn get_line_number(&self) -> u32 {
        self.state.current_line_number
    }

    pub fn single_indent(&mut self) {
        self.base_write(&self.single_indentation_text.clone());
    }

    pub fn write(&mut self, text: &str) {
        validate_text(text);
        self.base_write(text);

        fn validate_text(text: &str) {
            // todo: turn this off except when testing?
            if text.contains("\n") {
                panic!("Printer error: The IR generation should not write newlines. Use Signal.NewLine instead.");
            }
        }
    }

    pub fn new_line(&mut self) {
        self.base_write(&self.newline_kind.clone())
    }

    pub fn base_write(&mut self, text: &str) {
        let starts_with_new_line = text.starts_with("\n") || text.starts_with("\r\n");

        if self.state.expect_newline_next {
            self.state.expect_newline_next = false;
            if !starts_with_new_line {
                self.base_write(&self.newline_kind.clone());
                self.base_write(text);
                return;
            }
        }

        let mut text = String::from(text);
        if self.state.current_line_column == 0 && !starts_with_new_line && self.state.indent_level > 0 && self.state.ignore_indent_count == 0 {
            text.insert_str(0, &self.single_indentation_text.repeat(self.state.indent_level as usize));
        }

        for c in text.chars() {
            if c == '\n' {
                self.state.current_line_column = 0;
                self.state.current_line_number += 1;
                self.state.last_line_indent_level = self.state.indent_level;
            } else {
                // update the indent level again if on the first line
                if self.state.current_line_column == 0 {
                    self.state.last_line_indent_level = self.state.indent_level;
                }

                if c == '\t' {
                    self.state.current_line_column += self.indent_width as u32;
                } else {
                    self.state.current_line_column += 1;
                }
            }
        }

        self.state.items.push(text);
    }

    pub fn to_string(&self) -> String {
        self.state.items.join("")
    }
}

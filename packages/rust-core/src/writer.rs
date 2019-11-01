use super::StringRef;
use super::WriteItem;

#[derive(Clone)]
pub struct WriterState<T> where T : StringRef {
    pub current_line_column: u32,
    pub current_line_number: u32,
    pub last_line_indent_level: u16,
    pub indent_level: u16,
    pub expect_newline_next: bool,
    pub items: Vec<WriteItem<T>>,
    pub ignore_indent_count: u8,
}

pub struct WriterOptions {
    pub indent_width: u8,
}

pub struct Writer<T> where T : StringRef {
    state: WriterState<T>,
    indent_width: u8,
}

impl<T> Writer<T> where T : StringRef {
    pub fn new(options: WriterOptions) -> Writer<T> {
        Writer {
            indent_width: options.indent_width,
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

    pub fn get_state(&self) -> WriterState<T> {
        self.state.clone()
    }

    pub fn set_state(&mut self, state: WriterState<T>) {
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
        (self.indent_width as u32) * (self.state.last_line_indent_level as u32)
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

    pub fn new_line(&mut self) {
        self.state.current_line_column = 0;
        self.state.current_line_number += 1;
        self.state.last_line_indent_level = self.state.indent_level;
        self.state.expect_newline_next = false;
        self.state.items.push(WriteItem::NewLine);
    }

    pub fn single_indent(&mut self) {
        self.handle_first_column();
        self.state.current_line_column += self.indent_width as u32;
        self.state.items.push(WriteItem::Indent);
    }

    pub fn tab(&mut self) {
        self.handle_first_column();
        self.state.current_line_column += self.indent_width as u32;
        self.state.items.push(WriteItem::Tab);
    }

    pub fn space(&mut self) {
        self.handle_first_column();
        self.state.current_line_column += 1;
        self.state.items.push(WriteItem::Space);
    }

    pub fn write(&mut self, text: &T) {
        self.handle_first_column();
        self.state.current_line_column += text.get_length() as u32;
        self.state.items.push(WriteItem::String(text.clone()));
    }

    fn handle_first_column(&mut self) {
        if self.state.expect_newline_next {
            self.new_line();
        }

        // add the indentation if necessary
        if self.state.current_line_column == 0 && self.state.indent_level > 0 && self.state.ignore_indent_count == 0 {
            // update the indent level again if on the first column
            self.state.last_line_indent_level = self.state.indent_level;

            for _ in 0..self.state.indent_level {
                self.state.items.push(WriteItem::Indent);
            }

            self.state.current_line_column += self.state.indent_level as u32 * self.indent_width as u32;
        }
    }

    pub fn get_items(self) -> Vec<WriteItem<T>> {
        self.state.items
    }

    #[allow(dead_code)]
    pub fn get_items_copy(&self) -> Vec<WriteItem<T>> {
        self.state.items.clone()
    }
}

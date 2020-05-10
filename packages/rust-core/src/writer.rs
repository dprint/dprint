use super::StringContainer;
use super::WriteItem;
use super::collections::{GraphNode, GraphNodeIterator};
use super::print_items::WriterInfo;
use std::rc::Rc;

pub struct WriterState {
    current_line_column: u32,
    current_line_number: u32,
    last_line_indent_level: u8,
    indent_level: u8,
    expect_newline_next: bool,
    indent_queue_count: u8,
    last_was_not_trailing_space: bool,
    ignore_indent_count: u8,
    items: Option<Rc<GraphNode<WriteItem>>>,
}

impl WriterState {
    pub fn get_writer_info(&self, indent_width: u8) -> WriterInfo {
        WriterInfo {
            line_number: self.current_line_number,
            column_number: self.current_line_column,
            indent_level: self.indent_level,
            line_start_indent_level: self.last_line_indent_level,
            line_start_column_number: get_line_start_column_number(&self, indent_width),
        }
    }
}

impl Clone for WriterState {
    fn clone(&self) -> WriterState {
        WriterState {
            current_line_column: self.current_line_column,
            current_line_number: self.current_line_number,
            last_line_indent_level: self.last_line_indent_level,
            indent_level: self.indent_level,
            expect_newline_next: self.expect_newline_next,
            indent_queue_count: self.indent_queue_count,
            last_was_not_trailing_space: self.last_was_not_trailing_space,
            ignore_indent_count: self.ignore_indent_count,
            items: self.items.clone(),
        }
    }
}

pub struct WriterOptions {
    pub indent_width: u8,
}

pub struct Writer {
    state: WriterState,
    indent_width: u8,
}

impl Writer {
    pub fn new(options: WriterOptions) -> Writer {
        Writer {
            indent_width: options.indent_width,
            state: WriterState {
                current_line_column: 0,
                current_line_number: 0,
                last_line_indent_level: 0,
                indent_level: 0,
                expect_newline_next: false,
                indent_queue_count: 0,
                last_was_not_trailing_space: false,
                ignore_indent_count: 0,
                items: None,
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
        self.set_indent_level(self.state.indent_level + 1);
    }

    pub fn finish_indent(&mut self) {
        if self.state.indent_queue_count > 0 {
            self.state.indent_queue_count -= 1;
        } else {
            if self.state.indent_level == 0 {
                panic!("For some reason finish_indent was called without a corresponding start_indent.");
            }

            self.set_indent_level(self.state.indent_level - 1);
        }
    }

    fn set_indent_level(&mut self, new_level: u8) {
        self.state.indent_level = new_level;

        // If it's on the first column, update the indent level
        // that the line started on.
        if self.state.current_line_column == 0 {
            self.state.last_line_indent_level = new_level;
        }
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

    pub fn space_if_not_trailing(&mut self) {
        if !self.state.expect_newline_next {
            self.space();
            self.state.last_was_not_trailing_space = true;
        }
    }

    pub fn queue_indent(&mut self) {
        self.state.indent_queue_count += 1;
    }

    #[inline]
    pub fn get_line_start_indent_level(&self) -> u8 {
        self.state.last_line_indent_level
    }

    #[inline]
    pub fn get_indentation_level(&self) -> u8 {
        self.state.indent_level
    }

    #[inline]
    pub fn get_indent_width(&self) -> u8 {
        self.indent_width
    }

    #[cfg(debug_assertions)]
    pub fn get_ignore_indent_count(&self) -> u8 {
        self.state.ignore_indent_count
    }

    #[inline]
    pub fn get_line_start_column_number(&self) -> u32 {
        get_line_start_column_number(&self.state, self.indent_width)
    }

    #[inline]
    pub fn get_line_column(&self) -> u32 {
        if self.state.current_line_column == 0 {
            (self.indent_width as u32) * (self.state.indent_level as u32)
        } else {
            self.state.current_line_column
        }
    }

    #[inline]
    pub fn get_line_number(&self) -> u32 {
        self.state.current_line_number
    }

    pub fn new_line(&mut self) {
        if self.state.last_was_not_trailing_space {
            self.pop_item();
            self.state.last_was_not_trailing_space = false;
        }

        self.state.current_line_column = 0;
        self.state.current_line_number += 1;
        self.state.last_line_indent_level = self.state.indent_level;
        self.state.expect_newline_next = false;
        self.push_item(WriteItem::NewLine);
    }

    pub fn single_indent(&mut self) {
        self.handle_first_column();
        self.state.current_line_column += self.indent_width as u32;
        self.push_item(WriteItem::Indent(1));
    }

    pub fn tab(&mut self) {
        self.handle_first_column();
        self.state.current_line_column += self.indent_width as u32;
        self.push_item(WriteItem::Tab);
    }

    pub fn space(&mut self) {
        self.handle_first_column();
        self.state.current_line_column += 1;
        self.push_item(WriteItem::Space);
    }

    pub fn write(&mut self, text: Rc<StringContainer>) {
        self.handle_first_column();
        self.state.current_line_column += text.char_count;
        self.push_item(WriteItem::String(text));
    }

    fn handle_first_column(&mut self) {
        if self.state.expect_newline_next {
            self.new_line();
        }

        self.state.last_was_not_trailing_space = false;

        // add the indentation if necessary
        if self.state.current_line_column == 0 && self.state.indent_level > 0 && self.state.ignore_indent_count == 0 {
            // update the indent level again since on the first column
            self.state.last_line_indent_level = self.state.indent_level;

            if self.state.indent_level > 0 {
                self.push_item(WriteItem::Indent(self.state.indent_level));
            }

            self.state.current_line_column += self.state.indent_level as u32 * self.indent_width as u32;
        }
    }

    fn push_item(&mut self, item: WriteItem) {
        let previous = std::mem::replace(&mut self.state.items, None);
        self.state.items = Some(Rc::new(GraphNode::new(item, previous)));

        if self.state.indent_queue_count > 0 {
            let indent_count = self.state.indent_queue_count;
            self.state.indent_queue_count = 0;
            self.state.indent_level = self.state.indent_level + indent_count;
        }
    }

    fn pop_item(&mut self) {
        if let Some(previous) = &self.state.items {
            self.state.items = previous.borrow_previous().clone();
        }
    }

    pub fn get_items(self) -> impl Iterator<Item = WriteItem> {
        match self.state.items {
            Some(items) => Rc::try_unwrap(items).ok().expect("Expected to unwrap from RC at this point.").into_iter().collect::<Vec<_>>().into_iter().rev(),
            None => GraphNodeIterator::empty().collect::<Vec<_>>().into_iter().rev(),
        }
    }

    #[cfg(debug_assertions)]
    #[allow(dead_code)]
    pub fn to_string_for_debugging(&self) -> String {
        let write_items = self.get_items_cloned();
        super::print_write_items(write_items.into_iter(), super::PrintWriteItemsOptions {
            use_tabs: false,
            new_line_text: "\n",
            indent_width: 4,
        })
    }

    #[cfg(debug_assertions)]
    fn get_items_cloned(&self) -> Vec<WriteItem> {
        let mut items = Vec::new();
        let mut current_item = self.state.items.clone();
        while let Some(item) = current_item {
            // insert at the start since items are stored last to first
            items.insert(0, item.borrow_item().clone());
            current_item = item.borrow_previous().clone();
        }
        items
    }
}

#[inline]
fn get_line_start_column_number(writer_state: &WriterState, indent_width: u8) -> u32 {
    (writer_state.last_line_indent_level as u32) * (indent_width as u32)
}

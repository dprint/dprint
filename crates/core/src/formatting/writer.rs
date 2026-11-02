#[cfg(debug_assertions)]
use std::collections::VecDeque;

use super::Arena;
use super::StringContainer;
use super::WriteItem;
use super::collections::GraphNode;
use super::print_items::WriterInfo;

#[derive(Clone)]
pub struct WriterState<'a> {
  current_line_column: u32,
  current_line_number: u32,
  last_line_indent_level: u8,
  indent_level: u8,
  /// The columns of spaces after the indentation that the line started with.
  last_line_align_columns: u32,
  /// The columns of spaces written after the indentation.
  align_columns: u32,
  /// The alignments to restore when the sections that align finish.
  previous_aligns: Option<&'a PreviousAlign<'a>>,
  expect_newline_next: bool,
  indent_queue_count: u8,
  last_was_not_trailing_space: bool,
  ignore_indent_count: u8,
  /// The indentation to restore when the sections that reset the indentation finish.
  previous_reset_indents: Option<&'a PreviousResetIndent<'a>>,
  items: Option<&'a GraphNode<'a, WriteItem<'a>>>,
}

// these are linked lists so that cloning the writer state for a save point is cheap

/// The alignment from before a section that aligns.
struct PreviousAlign<'a> {
  align_columns: u32,
  previous: Option<&'a PreviousAlign<'a>>,
}

/// The indentation from before a section that resets the indentation.
struct PreviousResetIndent<'a> {
  indent_level: u8,
  indent_queue_count: u8,
  align_columns: u32,
  previous_aligns: Option<&'a PreviousAlign<'a>>,
  previous: Option<&'a PreviousResetIndent<'a>>,
}

impl WriterState<'_> {
  pub fn writer_info(&self, indent_width: u8) -> WriterInfo {
    WriterInfo {
      line_number: self.current_line_number,
      column_number: self.column_number(indent_width),
      indent_level: self.indent_level,
      line_start_indent_level: self.last_line_indent_level,
      line_start_align_columns: self.last_line_align_columns,
      indent_width,
      expect_newline_next: self.expect_newline_next,
    }
  }

  #[inline]
  pub fn column_number(&self, indent_width: u8) -> u32 {
    if self.current_line_column == 0 {
      (indent_width as u32) * (self.indent_level as u32) + self.align_columns
    } else {
      self.current_line_column
    }
  }
}

pub struct WriterOptions {
  pub indent_width: u8,
  #[cfg(feature = "tracing")]
  pub enable_tracing: bool,
}

pub struct Writer<'a> {
  arena: &'a Arena,
  state: WriterState<'a>,
  indent_width: u8,
  #[cfg(feature = "tracing")]
  nodes: Option<Vec<&'a GraphNode<'a, WriteItem<'a>>>>,
}

impl<'a> Writer<'a> {
  pub fn new(arena: &'a Arena, options: WriterOptions) -> Writer<'a> {
    Writer {
      arena,
      indent_width: options.indent_width,
      state: WriterState {
        current_line_column: 0,
        current_line_number: 0,
        last_line_indent_level: 0,
        indent_level: 0,
        last_line_align_columns: 0,
        align_columns: 0,
        previous_aligns: None,
        expect_newline_next: false,
        indent_queue_count: 0,
        last_was_not_trailing_space: false,
        ignore_indent_count: 0,
        previous_reset_indents: None,
        items: None,
      },
      #[cfg(feature = "tracing")]
      nodes: if options.enable_tracing { Some(Vec::new()) } else { None },
    }
  }

  pub fn writer_info(&self) -> WriterInfo {
    self.state.writer_info(self.indent_width)
  }

  pub fn state(&self) -> WriterState<'a> {
    self.state.clone()
  }

  pub fn set_state(&mut self, state: WriterState<'a>) {
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

  pub fn start_resetting_indent(&mut self) {
    let previous_reset_indent = self.arena.bump().alloc(PreviousResetIndent {
      indent_level: self.state.indent_level,
      indent_queue_count: self.state.indent_queue_count,
      align_columns: self.state.align_columns,
      previous_aligns: self.state.previous_aligns,
      previous: self.state.previous_reset_indents,
    });
    self.state.previous_reset_indents = Some(previous_reset_indent);
    // the queued indents are for the indentation outside, so they're applied after the section
    self.state.indent_queue_count = 0;
    self.state.previous_aligns = None;
    self.set_indent_level(0);
    self.set_align_columns(0);
  }

  pub fn finish_resetting_indent(&mut self) {
    let Some(previous_reset_indent) = self.state.previous_reset_indents else {
      panic!("For some reason finish_resetting_indent was called without a corresponding start_resetting_indent.");
    };
    if self.state.indent_level != 0 || self.state.indent_queue_count != 0 || self.state.previous_aligns.is_some() {
      panic!("For some reason finish_resetting_indent was called without finishing the indents or aligns started after start_resetting_indent.");
    }
    self.state.previous_reset_indents = previous_reset_indent.previous;
    self.state.indent_queue_count = previous_reset_indent.indent_queue_count;
    self.state.previous_aligns = previous_reset_indent.previous_aligns;
    self.set_indent_level(previous_reset_indent.indent_level);
    self.set_align_columns(previous_reset_indent.align_columns);
  }

  pub fn start_align(&mut self, columns: u8) {
    let previous_align = self.arena.bump().alloc(PreviousAlign {
      align_columns: self.state.align_columns,
      previous: self.state.previous_aligns,
    });
    self.state.previous_aligns = Some(previous_align);
    self.set_align_columns(self.state.align_columns + columns as u32);
  }

  pub fn finish_align(&mut self) {
    let Some(previous_align) = self.state.previous_aligns else {
      panic!("For some reason finish_align was called without a corresponding start_align.");
    };
    self.state.previous_aligns = previous_align.previous;
    self.set_align_columns(previous_align.align_columns);
  }

  fn set_align_columns(&mut self, columns: u32) {
    self.state.align_columns = columns;

    // if it's on the first column, update the alignment
    // that the line started with
    if self.state.current_line_column == 0 {
      self.state.last_line_align_columns = columns;
    }
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

  #[cfg(debug_assertions)]
  #[inline]
  pub fn indentation_level(&self) -> u8 {
    self.state.indent_level
  }

  #[inline]
  pub fn indent_width(&self) -> u8 {
    self.indent_width
  }

  #[cfg(debug_assertions)]
  pub fn ignore_indent_count(&self) -> u8 {
    self.state.ignore_indent_count
  }

  #[cfg(debug_assertions)]
  pub fn is_resetting_indent(&self) -> bool {
    self.state.previous_reset_indents.is_some()
  }

  #[cfg(debug_assertions)]
  pub fn is_aligning(&self) -> bool {
    self.state.previous_aligns.is_some()
  }

  #[inline]
  pub fn column_number(&self) -> u32 {
    self.state.column_number(self.indent_width)
  }

  #[inline]
  pub fn line_number(&self) -> u32 {
    self.state.current_line_number
  }

  #[inline]
  pub fn is_start_of_line(&self) -> bool {
    self.state.expect_newline_next || self.column_number() == self.line_start_column_number()
  }

  #[inline]
  pub fn indent_level(&self) -> u8 {
    self.state.indent_level
  }

  #[inline]
  pub fn line_start_column_number(&self) -> u32 {
    (self.state.last_line_indent_level as u32) * (self.indent_width as u32) + self.state.last_line_align_columns
  }

  #[inline]
  pub fn line_start_indent_level(&self) -> u8 {
    self.state.last_line_indent_level
  }

  pub fn new_line(&mut self) {
    if self.state.last_was_not_trailing_space {
      self.pop_item();
      self.state.last_was_not_trailing_space = false;
    }

    self.state.current_line_column = 0;
    self.state.current_line_number += 1;
    self.state.last_line_indent_level = self.state.indent_level;
    self.state.last_line_align_columns = self.state.align_columns;
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

  fn space(&mut self) {
    self.handle_first_column();
    self.state.current_line_column += 1;
    self.push_item(WriteItem::Space);
  }

  pub fn write(&mut self, text: &'a StringContainer<'a>) {
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
    if self.state.current_line_column == 0 && (self.state.indent_level > 0 || self.state.align_columns > 0) && self.state.ignore_indent_count == 0 {
      // update the indentation again since on the first column
      self.state.last_line_indent_level = self.state.indent_level;
      self.state.last_line_align_columns = self.state.align_columns;

      // set the current line column
      self.state.current_line_column = self.state.indent_level as u32 * self.indent_width as u32 + self.state.align_columns;

      // finally, push the indentation, which is the indent level followed by the alignment
      let align_columns = self.state.align_columns;
      if self.state.indent_level > 0 {
        // this might update the indent_level based on the queued indentation, so do this last
        self.push_item(WriteItem::Indent(self.state.indent_level));
      }
      if align_columns > 0 {
        self.push_item(WriteItem::Align(align_columns));
      }
    }
  }

  fn push_item(&mut self, item: WriteItem<'a>) {
    let previous = self.state.items.take();
    let graph_node = self.arena.alloc_write_item_graph_node(GraphNode::new(item, previous));
    self.state.items = Some(graph_node);

    #[cfg(feature = "tracing")]
    if let Some(nodes) = self.nodes.as_mut() {
      nodes.push(graph_node);
    }

    if self.state.indent_queue_count > 0 {
      let indent_count = self.state.indent_queue_count;
      self.state.indent_queue_count = 0;
      self.state.indent_level += indent_count;
    }
  }

  fn pop_item(&mut self) {
    if let Some(previous) = &self.state.items {
      self.state.items = previous.previous;
    }
  }

  pub fn items(self) -> Option<impl Iterator<Item = WriteItem<'a>>> {
    self.state.items.map(|items| items.iter().rev())
  }

  #[cfg(feature = "tracing")]
  pub fn current_node_id(&self) -> Option<u32> {
    self.state.items.as_ref().map(|node| node.graph_node_id)
  }

  #[cfg(feature = "tracing")]
  pub fn nodes(self) -> Vec<&'a GraphNode<'a, WriteItem<'a>>> {
    self.nodes.expect("Should have set enable_tracing to true.")
  }

  #[cfg(debug_assertions)]
  #[allow(dead_code)]
  pub fn to_string_for_debugging(&self) -> String {
    use super::WriteItemsPrinter;
    let write_items = self.items_cloned();
    WriteItemsPrinter::new(self.indent_width, false, "\n").print(write_items.into_iter())
  }

  #[cfg(debug_assertions)]
  fn items_cloned(&self) -> VecDeque<WriteItem<'_>> {
    let mut items = VecDeque::new();
    let mut current_item = self.state.items;
    while let Some(item) = current_item {
      // insert at the start since items are stored last to first
      items.push_front(item.item);
      current_item = item.previous;
    }
    items
  }
}

#[cfg(test)]
mod test {
  use crate::formatting::Arena;

  use super::super::Indentation;
  use super::super::WriteItemsPrinter;
  use super::*;

  // todo: some basic unit tests just to make sure I'm not way off

  #[test]
  fn write_singleword_writes() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    write_text(&mut writer, "test", &arena);
    assert_writer_equal(writer, "test");
  }

  #[test]
  fn write_multiple_lines_writes() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    write_text(&mut writer, "1", &arena);
    writer.new_line();
    write_text(&mut writer, "2", &arena);
    assert_writer_equal(writer, "1\n2");
  }

  #[test]
  fn write_indented_writes() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    write_text(&mut writer, "1", &arena);
    writer.new_line();
    writer.start_indent();
    write_text(&mut writer, "2", &arena);
    writer.finish_indent();
    writer.new_line();
    write_text(&mut writer, "3", &arena);
    assert_writer_equal(writer, "1\n  2\n3");
  }

  #[test]
  fn write_reset_indent_writes() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    writer.start_indent();
    writer.start_indent();
    write_text(&mut writer, "1", &arena);
    writer.start_resetting_indent();
    writer.new_line();
    write_text(&mut writer, "2", &arena);
    writer.start_indent();
    writer.new_line();
    write_text(&mut writer, "3", &arena);
    // nested
    writer.start_resetting_indent();
    writer.new_line();
    write_text(&mut writer, "4", &arena);
    writer.finish_resetting_indent();
    writer.new_line();
    write_text(&mut writer, "5", &arena);
    writer.finish_indent();
    writer.finish_resetting_indent();
    writer.new_line();
    write_text(&mut writer, "6", &arena);
    writer.finish_indent();
    writer.finish_indent();
    assert_writer_equal(writer, "    1\n2\n  3\n4\n  5\n    6");
  }

  #[test]
  fn write_queued_indent_after_reset_indent() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    writer.queue_indent();
    writer.start_resetting_indent();
    writer.start_indent();
    write_text(&mut writer, "1", &arena);
    writer.new_line();
    write_text(&mut writer, "2", &arena);
    writer.finish_indent();
    writer.finish_resetting_indent();
    // the queued indent applies after the next item written outside the section
    write_text(&mut writer, "3", &arena);
    writer.new_line();
    write_text(&mut writer, "4", &arena);
    writer.finish_indent();
    assert_writer_equal(writer, "  1\n  23\n  4");
  }

  #[test]
  fn write_align_writes() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    write_text(&mut writer, "1", &arena);
    writer.start_align(3);
    writer.new_line();
    write_text(&mut writer, "2", &arena);
    assert_eq!(writer.column_number(), 4);
    assert_eq!(writer.line_start_column_number(), 3);
    writer.start_indent();
    writer.start_align(1);
    writer.new_line();
    // the indentation goes before the alignment
    write_text(&mut writer, "3", &arena);
    writer.start_resetting_indent();
    writer.new_line();
    write_text(&mut writer, "4", &arena);
    writer.finish_resetting_indent();
    writer.finish_align();
    writer.new_line();
    write_text(&mut writer, "5", &arena);
    writer.finish_indent();
    writer.finish_align();
    writer.new_line();
    write_text(&mut writer, "6", &arena);
    assert_writer_equal(writer, "1\n   2\n      3\n4\n     5\n6");
  }

  #[test]
  fn write_align_with_tabs_writes() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    writer.start_indent();
    writer.start_align(3);
    write_text(&mut writer, "1", &arena);
    writer.finish_align();
    writer.finish_indent();
    let p = WriteItemsPrinter {
      indent: Indentation::Tabs,
      newline: "\n",
    };
    assert_eq!(p.print(writer.items().unwrap()), "\t   1");
  }

  #[test]
  #[should_panic(expected = "without a corresponding start_align")]
  fn finish_align_panics_across_reset_indent() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    writer.start_align(2);
    writer.start_resetting_indent();
    writer.finish_align();
  }

  #[test]
  #[should_panic(expected = "without finishing the indents")]
  fn finish_reset_indent_panics_with_unfinished_indent() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    writer.start_resetting_indent();
    writer.start_indent();
    writer.finish_resetting_indent();
  }

  #[test]
  fn write_singleindent_writes() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    writer.single_indent();
    write_text(&mut writer, "t", &arena);
    assert_writer_equal(writer, "  t");
  }

  #[test]
  fn markexpectnewline_writesnewline() {
    let arena = Arena::new();
    let mut writer = create_writer(&arena);
    write_text(&mut writer, "1", &arena);
    writer.mark_expect_new_line();
    write_text(&mut writer, "2", &arena);
    assert_writer_equal(writer, "1\n2");
  }

  fn assert_writer_equal(writer: Writer, text: &str) {
    let p = WriteItemsPrinter {
      indent: Indentation::Spaces(2),
      newline: "\n",
    };
    assert_eq!(p.print(writer.items().unwrap()), String::from(text));
  }

  fn write_text<'a>(writer: &mut Writer<'a>, text: &'static str, arena: &'a Arena) {
    let string_container = arena.alloc_string_container(text);
    writer.write(string_container);
  }

  fn create_writer(arena: &Arena) -> Writer<'_> {
    Writer::new(
      arena,
      WriterOptions {
        indent_width: 2,
        #[cfg(feature = "tracing")]
        enable_tracing: false,
      },
    )
  }
}

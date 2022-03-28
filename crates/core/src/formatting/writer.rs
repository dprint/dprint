use super::collections::GraphNode;
use super::print_items::WriterInfo;
use super::StringContainer;
use super::WriteItem;
use bumpalo::Bump;

#[derive(Clone)]
pub struct WriterState<'a> {
  current_line_column: u32,
  current_line_number: u32,
  last_line_indent_level: u8,
  indent_level: u8,
  expect_newline_next: bool,
  indent_queue_count: u8,
  last_was_not_trailing_space: bool,
  ignore_indent_count: u8,
  items: Option<&'a GraphNode<'a, WriteItem<'a>>>,
}

impl<'a> WriterState<'a> {
  pub fn get_writer_info(&self, indent_width: u8) -> WriterInfo {
    WriterInfo {
      line_number: self.current_line_number,
      column_number: self.get_line_column(indent_width),
      indent_level: self.indent_level,
      line_start_indent_level: self.last_line_indent_level,
      indent_width,
    }
  }

  #[inline]
  pub fn get_line_column(&self, indent_width: u8) -> u32 {
    if self.current_line_column == 0 {
      (indent_width as u32) * (self.indent_level as u32)
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
  bump: &'a Bump,
  state: WriterState<'a>,
  indent_width: u8,
  #[cfg(feature = "tracing")]
  nodes: Option<Vec<&'a GraphNode<'a, WriteItem<'a>>>>,
}

impl<'a> Writer<'a> {
  pub fn new(bump: &'a Bump, options: WriterOptions) -> Writer<'a> {
    Writer {
      bump,
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
      #[cfg(feature = "tracing")]
      nodes: if options.enable_tracing { Some(Vec::new()) } else { None },
    }
  }

  pub fn get_writer_info(&self) -> WriterInfo {
    self.state.get_writer_info(self.indent_width)
  }

  pub fn get_state(&self) -> WriterState<'a> {
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
  pub fn get_line_column(&self) -> u32 {
    self.state.get_line_column(self.indent_width)
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

  fn space(&mut self) {
    self.handle_first_column();
    self.state.current_line_column += 1;
    self.push_item(WriteItem::Space);
  }

  pub fn write(&mut self, text: &'a StringContainer) {
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

      // set the current line column
      self.state.current_line_column = self.state.indent_level as u32 * self.indent_width as u32;

      // finally, push the indent level
      if self.state.indent_level > 0 {
        // this might update the indent_level based on the queued indentation, so do this last
        self.push_item(WriteItem::Indent(self.state.indent_level));
      }
    }
  }

  fn push_item(&mut self, item: WriteItem<'a>) {
    let previous = std::mem::replace(&mut self.state.items, None);
    let graph_node = self.bump.alloc(GraphNode::new(item, previous));
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
  pub fn get_current_node_id(&self) -> Option<usize> {
    self.state.items.as_ref().map(|node| node.graph_node_id)
  }

  #[cfg(feature = "tracing")]
  pub fn get_nodes(self) -> Vec<&'a GraphNode<'a, WriteItem<'a>>> {
    self.nodes.expect("Should have set enable_tracing to true.")
  }

  #[cfg(debug_assertions)]
  #[allow(dead_code)]
  pub fn to_string_for_debugging(&self) -> String {
    use super::WriteItemsPrinter;
    let write_items = self.get_items_cloned();
    WriteItemsPrinter::new(self.indent_width, false, "\n").print(write_items.into_iter())
  }

  #[cfg(debug_assertions)]
  fn get_items_cloned(&self) -> Vec<WriteItem> {
    let mut items = Vec::new();
    let mut current_item = self.state.items;
    while let Some(item) = current_item {
      // insert at the start since items are stored last to first
      items.insert(0, item.item);
      current_item = item.previous;
    }
    items
  }
}

#[cfg(test)]
mod test {
  use super::super::utils::with_bump_allocator_mut;
  use super::super::Indentation;
  use super::super::StringContainer;
  use super::super::WriteItemsPrinter;
  use super::*;

  // todo: some basic unit tests just to make sure I'm not way off

  #[test]
  fn write_singleword_writes() {
    with_bump_allocator_mut(|bump| {
      let mut writer = create_writer(bump);
      write_text(&mut writer, "test", bump);
      assert_writer_equal(writer, "test");
      bump.reset();
    });
  }

  #[test]
  fn write_multiple_lines_writes() {
    with_bump_allocator_mut(|bump| {
      let mut writer = create_writer(bump);
      write_text(&mut writer, "1", bump);
      writer.new_line();
      write_text(&mut writer, "2", bump);
      assert_writer_equal(writer, "1\n2");
      bump.reset();
    });
  }

  #[test]
  fn write_indented_writes() {
    with_bump_allocator_mut(|bump| {
      let mut writer = create_writer(bump);
      write_text(&mut writer, "1", bump);
      writer.new_line();
      writer.start_indent();
      write_text(&mut writer, "2", bump);
      writer.finish_indent();
      writer.new_line();
      write_text(&mut writer, "3", bump);
      assert_writer_equal(writer, "1\n  2\n3");
      bump.reset();
    });
  }

  #[test]
  fn write_singleindent_writes() {
    with_bump_allocator_mut(|bump| {
      let mut writer = create_writer(bump);
      writer.single_indent();
      write_text(&mut writer, "t", bump);
      assert_writer_equal(writer, "  t");
      bump.reset();
    });
  }

  #[test]
  fn markexpectnewline_writesnewline() {
    with_bump_allocator_mut(|bump| {
      let mut writer = create_writer(bump);
      write_text(&mut writer, "1", bump);
      writer.mark_expect_new_line();
      write_text(&mut writer, "2", bump);
      assert_writer_equal(writer, "1\n2");
      bump.reset();
    });
  }

  fn assert_writer_equal(writer: Writer, text: &str) {
    let p = WriteItemsPrinter {
      indent: Indentation::Spaces(2),
      newline: "\n",
    };
    assert_eq!(p.print(writer.items().unwrap()), String::from(text));
  }

  fn write_text(writer: &mut Writer, text: &'static str, bump: &Bump) {
    let string_container = {
      let result = bump.alloc(StringContainer::new(String::from(text)));
      unsafe { std::mem::transmute::<&StringContainer, &'static StringContainer>(result) }
    };
    writer.write(string_container);
  }

  fn create_writer(bump: &Bump) -> Writer {
    Writer::new(
      bump,
      WriterOptions {
        indent_width: 2,
        #[cfg(feature = "tracing")]
        enable_tracing: false,
      },
    )
  }
}

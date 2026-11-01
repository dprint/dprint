use std::cell::UnsafeCell;
use std::mem;

use super::Arena;
use super::condition_resolvers;
use super::printer::Printer;
use super::thread_state;

/// A list of print items allocated in an [`Arena`].
pub struct PrintItems<'a> {
  arena: &'a Arena,
  pub(super) first_node: Option<PrintItemPath<'a>>,
  last_node: Option<PrintItemPath<'a>>,
}

impl<'a> PrintItems<'a> {
  pub fn new(arena: &'a Arena) -> Self {
    Self {
      arena,
      first_node: None,
      last_node: None,
    }
  }

  /// Gets the arena the print items are allocated in.
  #[inline]
  pub fn arena(&self) -> &'a Arena {
    self.arena
  }

  pub fn into_rc_path(self) -> Option<PrintItemPath<'a>> {
    self.first_node
  }

  pub fn push_item(&mut self, item: PrintItem<'a>) {
    self.push_item_internal(item);
  }

  #[inline]
  fn push_item_internal(&mut self, item: PrintItem<'a>) {
    let node = self.arena.alloc_print_node_cell(PrintNodeCell::new(item));
    if let Some(first_node) = &self.first_node {
      let new_last_node = node.get_last_next().unwrap_or(node);
      self.last_node.as_ref().unwrap_or(first_node).set_next(Some(node));
      self.last_node = Some(new_last_node);
    } else {
      self.last_node = node.get_last_next();
      self.first_node = Some(node);
    }
  }

  pub fn extend(&mut self, items: PrintItems<'a>) {
    if let Some(first_node) = items.first_node {
      if let Some(current_first_node) = &self.first_node {
        self.last_node.as_ref().unwrap_or(current_first_node).set_next(Some(first_node));

        if items.last_node.is_some() {
          self.last_node = items.last_node;
        } else if items.first_node.is_some() {
          self.last_node = items.first_node;
        }
      } else {
        self.first_node = items.first_node;
        self.last_node = items.last_node;
      }
    }
  }

  /// Pushes a string that will have its width computed at runtime.
  ///
  /// The string isn't copied, so this can be used for text that lives at least
  /// as long as the arena, such as the text of the file being formatted.
  ///
  /// Instead of using this, prefer to use `push_sc` where possible
  /// with the `sc!` macro from the dprint-core-macros crate.
  pub fn push_str_runtime_width_computed(&mut self, item: &'a str) {
    let string_container = self.arena.alloc_string_container(item);
    self.push_item_internal(PrintItem::String(string_container));
  }

  pub fn push_force_current_line_indentation(&mut self) {
    const STR_EMPTY: StringContainer = StringContainer { text: "", char_count: 0 };
    self.push_item_internal(PrintItem::String(&STR_EMPTY))
  }

  pub fn push_space(&mut self) {
    const STR_SPACE: StringContainer = StringContainer { text: " ", char_count: 1 };
    self.push_item_internal(PrintItem::String(&STR_SPACE))
  }

  /// Pushes a string that's had its width computed at compile time.
  pub fn push_sc(&mut self, item: &'a StringContainer<'a>) {
    self.push_item_internal(PrintItem::String(item))
  }

  /// Pushes a string, copying it into the arena.
  pub fn push_string(&mut self, item: String) {
    self.push_str(&item);
  }

  /// Pushes a borrowed string, copying it into the arena, and computes its width at
  /// runtime.
  ///
  /// Prefer `push_sc` with the `sc!` macro for text known at compile time, and
  /// `push_str_runtime_width_computed` for text that lives as long as the arena, since
  /// neither copies. Use this for other text the caller doesn't own: it avoids building
  /// a `String` only for it to be copied and thrown away.
  pub fn push_str(&mut self, item: &str) {
    let string_container = self.arena.alloc_str(item);
    self.push_item_internal(PrintItem::String(string_container));
  }

  pub fn push_condition(&mut self, condition: Condition<'a>) {
    let condition = self.arena.alloc_condition(condition);
    self.push_item_internal(PrintItem::Condition(condition));
  }

  pub fn push_info(&mut self, info: impl Into<Info>) {
    self.push_item_internal(PrintItem::Info(info.into()));
  }

  pub fn push_line_and_column(&mut self, line_and_col: LineAndColumn) {
    self.push_info(line_and_col.line);
    self.push_info(line_and_col.column);
  }

  pub fn push_anchor(&mut self, anchor: impl Into<Anchor>) {
    self.push_item_internal(PrintItem::Anchor(anchor.into()));
  }

  pub fn push_reevaluation(&mut self, condition_reevaluation: ConditionReevaluation) {
    self.push_item_internal(PrintItem::ConditionReevaluation(condition_reevaluation));
  }

  pub fn push_signal(&mut self, signal: Signal) {
    self.push_item_internal(PrintItem::Signal(signal));
  }

  pub fn push_path(&mut self, path: PrintItemPath<'a>) {
    self.push_item_internal(PrintItem::RcPath(path))
  }

  pub fn push_optional_path(&mut self, path: Option<PrintItemPath<'a>>) {
    if let Some(path) = path {
      self.push_path(path);
    }
  }

  pub fn is_empty(&self) -> bool {
    self.first_node.is_none()
  }

  // todo: clean this up
  #[cfg(debug_assertions)]
  pub fn get_as_text(&self) -> String {
    return if let Some(first_node) = &self.first_node {
      get_items_as_text(first_node, String::from(""))
    } else {
      String::new()
    };

    fn get_items_as_text(items: PrintItemPath, indent_text: String) -> String {
      let mut text = String::new();
      for item in PrintItemsIterator::new(items) {
        match item {
          PrintItem::Signal(signal) => text.push_str(&get_line(format!("Signal::{:?}", signal), &indent_text)),
          PrintItem::Condition(condition) => {
            text.push_str(&get_line(format!("Condition: {}", condition.name), &indent_text));
            if let Some(true_path) = &condition.true_path {
              text.push_str(&get_line(String::from("  true:"), &indent_text));
              text.push_str(&get_items_as_text(true_path, format!("{}    ", indent_text)));
            }
            if let Some(false_path) = &condition.false_path {
              text.push_str(&get_line(String::from("  false:"), &indent_text));
              text.push_str(&get_items_as_text(false_path, format!("{}    ", indent_text)));
            }
          }
          PrintItem::String(str_text) => text.push_str(&get_line(format!("`{}`", str_text.text), &indent_text)),
          PrintItem::RcPath(path) => text.push_str(&get_items_as_text(path, indent_text.clone())),
          PrintItem::Anchor(Anchor::LineNumber(line_number_anchor)) => {
            text.push_str(&get_line(format!("Line number anchor: {}", line_number_anchor.name()), &indent_text))
          }
          PrintItem::Info(info) => {
            let (desc, name) = match info {
              Info::LineNumber(info) => ("Line number", info.name()),
              Info::ColumnNumber(info) => ("Column number", info.name()),
              Info::IsStartOfLine(info) => ("Is start of line", info.name()),
              Info::IndentLevel(info) => ("Indent level", info.name()),
              Info::LineStartColumnNumber(info) => ("Line start column number", info.name()),
              Info::LineStartIndentLevel(info) => ("Line start indent level", info.name()),
            };
            text.push_str(&get_line(format!("{}: {}", desc, name), &indent_text))
          }
          PrintItem::ConditionReevaluation(reevaluation) => text.push_str(&get_line(format!("Condition reevaluation: {}", reevaluation.name()), &indent_text)),
        }
      }

      return text;

      fn get_line(text: String, indent_text: &str) -> String {
        format!("{}{}\n", indent_text, text)
      }
    }
  }

  pub fn iter(&self) -> PrintItemsIterator<'a> {
    PrintItemsIterator { node: self.first_node }
  }
}

pub struct PrintItemsIterator<'a> {
  node: Option<PrintItemPath<'a>>,
}

impl<'a> PrintItemsIterator<'a> {
  pub fn new(path: PrintItemPath<'a>) -> Self {
    Self { node: Some(path) }
  }
}

impl<'a> Iterator for PrintItemsIterator<'a> {
  type Item = PrintItem<'a>;

  fn next(&mut self) -> Option<PrintItem<'a>> {
    let node = self.node.take();

    match node {
      Some(node) => {
        self.node = node.get_next();
        Some(node.get_item())
      }
      None => None,
    }
  }
}

/// Converts a value into print items allocated in an arena.
pub trait IntoPrintItems<'a> {
  fn into_print_items(self, arena: &'a Arena) -> PrintItems<'a>;
}

impl<'a> IntoPrintItems<'a> for PrintItems<'a> {
  fn into_print_items(self, _arena: &'a Arena) -> PrintItems<'a> {
    self
  }
}

impl<'a> IntoPrintItems<'a> for &'a str {
  fn into_print_items(self, arena: &'a Arena) -> PrintItems<'a> {
    let mut items = PrintItems::new(arena);
    items.push_str_runtime_width_computed(self);
    items
  }
}

impl<'a> IntoPrintItems<'a> for &'a StringContainer<'a> {
  fn into_print_items(self, arena: &'a Arena) -> PrintItems<'a> {
    let mut items = PrintItems::new(arena);
    items.push_sc(self);
    items
  }
}

impl<'a> IntoPrintItems<'a> for String {
  fn into_print_items(self, arena: &'a Arena) -> PrintItems<'a> {
    let mut items = PrintItems::new(arena);
    items.push_string(self);
    items
  }
}

impl<'a> IntoPrintItems<'a> for Condition<'a> {
  fn into_print_items(self, arena: &'a Arena) -> PrintItems<'a> {
    let mut items = PrintItems::new(arena);
    items.push_condition(self);
    items
  }
}

impl<'a> IntoPrintItems<'a> for Signal {
  fn into_print_items(self, arena: &'a Arena) -> PrintItems<'a> {
    let mut items = PrintItems::new(arena);
    items.push_signal(self);
    items
  }
}

impl<'a> IntoPrintItems<'a> for PrintItemPath<'a> {
  fn into_print_items(self, arena: &'a Arena) -> PrintItems<'a> {
    let mut items = PrintItems::new(arena);
    items.push_path(self);
    items
  }
}

impl<'a, T: IntoPrintItems<'a>> IntoPrintItems<'a> for Option<T> {
  fn into_print_items(self, arena: &'a Arena) -> PrintItems<'a> {
    match self {
      Some(value) => value.into_print_items(arena),
      None => PrintItems::new(arena),
    }
  }
}

/** Tracing */

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trace {
  /// The relative time of the trace from the start of printing in nanoseconds.
  pub nanos: u128,
  pub print_node_id: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub writer_node_id: Option<u32>,
  /// Why printing the node made the printer go back to an earlier point, if it did.
  ///
  /// The printer continues from the print node of the next trace.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub jump: Option<TraceJump>,
}

/// Why the printer went back to an earlier point.
#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
pub enum TraceJump {
  /// The line was too long, so the printer went back to the last
  /// possible newline to write a newline there.
  NewLine,
  /// An info was printed after a condition used it before it was resolved.
  Info(TraceInfoJump),
  /// A condition was printed after another condition used its value
  /// before it was resolved.
  Condition(TraceConditionJump),
  /// A condition was re-evaluated and its value changed.
  ConditionReevaluation(TraceConditionJump),
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceInfoJump {
  pub info: TraceInfo,
  /// The value the info resolved to.
  pub value: TraceInfoValue,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(untagged)]
pub enum TraceInfoValue {
  Number(u32),
  Bool(bool),
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceConditionJump {
  pub condition_id: u32,
  pub name: String,
  /// The value the condition resolved to.
  pub value: Option<bool>,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceWriterNode {
  pub writer_node_id: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub previous_node_id: Option<u32>,
  pub text: String,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracePrintNode {
  pub print_node_id: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub next_print_node_id: Option<u32>,
  pub print_item: TracePrintItem,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
pub enum TracePrintItem {
  String(String),
  Condition(TraceCondition),
  Info(TraceInfo),
  Signal(Signal),
  /// Identifier to the print node.
  RcPath(u32),
  Anchor(TraceLineNumberAnchor),
  ConditionReevaluation(TraceConditionReevaluation),
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
pub enum TraceInfo {
  LineNumber(TraceInfoInner),
  ColumnNumber(TraceInfoInner),
  IsStartOfLine(TraceInfoInner),
  IndentLevel(TraceInfoInner),
  LineStartColumnNumber(TraceInfoInner),
  LineStartIndentLevel(TraceInfoInner),
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceInfoInner {
  pub info_id: u32,
  pub name: String,
}

#[cfg(feature = "tracing")]
impl TraceInfoInner {
  pub fn new(info_id: u32, name: &str) -> Self {
    Self {
      info_id,
      name: name.to_string(),
    }
  }
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceLineNumberAnchor {
  pub anchor_id: u32,
  pub name: String,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceConditionReevaluation {
  pub condition_id: u32,
  pub name: String,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceCondition {
  pub condition_id: u32,
  pub name: String,
  pub is_stored: bool,
  pub store_save_point: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  /// Identifier to the true path print node.
  pub true_path: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  /// Identifier to the false path print node.
  pub false_path: Option<u32>,
}

pub struct PrintNode<'a> {
  pub(super) next: Option<PrintItemPath<'a>>,
  pub(super) item: PrintItem<'a>,
  #[cfg(feature = "tracing")]
  pub print_node_id: u32,
}

impl<'a> PrintNode<'a> {
  fn new(item: PrintItem<'a>) -> PrintNode<'a> {
    PrintNode {
      item,
      next: None,
      #[cfg(feature = "tracing")]
      print_node_id: thread_state::next_print_node_id(),
    }
  }

  fn set_next(&mut self, new_next: Option<PrintItemPath<'a>>) {
    let past_next = mem::replace(&mut self.next, new_next);

    if let Some(past_next) = past_next
      && let Some(new_next) = new_next
    {
      new_next.get_last_next().unwrap_or(new_next).set_next(Some(past_next));
    }
  }
}

/// A fast implementation of RefCell<PrintNode> that avoids runtime checks on borrows.
pub struct PrintNodeCell<'a> {
  value: UnsafeCell<PrintNode<'a>>,
}

impl<'a> PrintNodeCell<'a> {
  pub(super) fn new(item: PrintItem<'a>) -> PrintNodeCell<'a> {
    PrintNodeCell {
      value: UnsafeCell::new(PrintNode::new(item)),
    }
  }

  #[inline]
  pub(super) fn get_item(&self) -> PrintItem<'a> {
    unsafe { (*self.value.get()).item.clone() }
  }

  #[inline]
  pub(super) fn get_next(&self) -> Option<PrintItemPath<'a>> {
    unsafe { (*self.value.get()).next }
  }

  #[inline]
  pub(super) fn set_next(&self, new_next: Option<PrintItemPath<'a>>) {
    unsafe {
      (*self.value.get()).set_next(new_next);
    }
  }

  #[inline]
  pub(super) fn get_last_next(&self) -> Option<PrintItemPath<'a>> {
    let mut current = self.get_next();
    loop {
      if let Some(last) = &current
        && let Some(next) = last.get_next()
      {
        current.replace(next);
        continue;
      }
      break;
    }

    current
  }

  #[cfg(feature = "tracing")]
  pub(super) fn get_node_id(&self) -> u32 {
    unsafe { (*self.get_node()).print_node_id }
  }

  /// Gets the node unsafely. Be careful when using this and ensure no mutation is
  /// happening during the borrow.
  #[inline]
  pub(super) unsafe fn get_node(&self) -> *mut PrintNode<'a> {
    self.value.get()
  }

  #[inline]
  pub fn take_next(self) -> Option<PrintItemPath<'a>> {
    self.value.into_inner().next.take()
  }
}

pub type PrintItemPath<'a> = &'a PrintNodeCell<'a>;

/* Print item and kinds */

/// The different items the printer could encounter.
#[derive(Clone)]
pub enum PrintItem<'a> {
  String(&'a StringContainer<'a>),
  Condition(&'a Condition<'a>),
  Signal(Signal),
  RcPath(PrintItemPath<'a>),
  Anchor(Anchor),
  Info(Info),
  ConditionReevaluation(ConditionReevaluation),
}

#[derive(Clone, PartialEq, Eq, Copy, Debug, serde::Serialize)]
pub enum Signal {
  /// Signal that a new line should occur based on the printer settings.
  NewLine,
  /// Signal that a tab should occur based on the printer settings.
  Tab,
  /// Signal that the current location could be a newline when
  /// exceeding the line width.
  PossibleNewLine,
  /// Signal that the current location should be a space, but
  /// could be a newline if exceeding the line width.
  SpaceOrNewLine,
  /// Expect the next character to be a newline. If it's not, force a newline.
  ExpectNewLine,
  /// Queue a start indent to be set after the next written item.
  QueueStartIndent,
  /// Signal the start of a section that should be indented.
  StartIndent,
  /// Signal the end of a section that should be indented.
  FinishIndent,
  /// Signal the start of a group of print items that have a lower precedence
  /// for being broken up with a newline for exceeding the line width.
  StartNewLineGroup,
  /// Signal the end of a newline group.
  FinishNewLineGroup,
  /// Signal that a single indent should occur based on the printer settings.
  SingleIndent,
  /// Signal to the printer that it should stop using indentation.
  StartIgnoringIndent,
  /// Signal to the printer that it should start using indentation again.
  FinishIgnoringIndent,
  /// Signal the start of a section whose indentation starts from no indentation
  /// instead of the current indentation, which can then be indented with `StartIndent`
  /// (ex. to indent relative to text written as is while ignoring the indentation).
  ///
  /// The indent levels in the section (ex. of an `IndentLevel` info) count from the
  /// start of the section, so they shouldn't be compared with the indent levels from
  /// outside of it. The column numbers are the actual columns. A `QueueStartIndent`
  /// from before the section is applied after it.
  StartResettingIndent,
  /// Signal the end of a section that reset the indentation, which restores the
  /// indentation from before. The indents and aligns started in the section must be
  /// finished first.
  FinishResettingIndent,
  /// Signal the start of a section that's aligned by the number of columns of spaces
  /// after the indentation (ex. to line up with text that isn't at an indent level).
  /// The alignment always goes after the indentation, so with tabs it's tabs then spaces.
  ///
  /// The alignment isn't part of the indent levels (ex. of an `IndentLevel` info),
  /// but it's included in the column numbers.
  StartAlign(u8),
  /// Signal the end of the most recent section that aligns.
  FinishAlign,
  /// Signal to the printer that it shouldn't print any new lines.
  StartForceNoNewLines,
  /// Signal to the printer that it should finish not printing any new lines.
  FinishForceNoNewLines,
  /// Signal that a space should occur if not trailing.
  SpaceIfNotTrailing,
}

#[derive(Clone)]
pub enum Anchor {
  LineNumber(LineNumberAnchor),
}

impl From<LineNumberAnchor> for Anchor {
  fn from(anchor: LineNumberAnchor) -> Self {
    Anchor::LineNumber(anchor)
  }
}

/// Handles updating the position of a future resolved line
/// number if the anchor changes.
#[derive(Clone)]
pub struct LineNumberAnchor {
  id: u32,
  line_number: LineNumber,
}

impl LineNumberAnchor {
  pub fn new(line_number: LineNumber) -> Self {
    Self {
      id: thread_state::next_line_number_anchor_id(),
      line_number,
    }
  }

  #[inline]
  pub fn unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn line_number_id(&self) -> u32 {
    self.line_number.id
  }

  #[inline]
  pub fn name(&self) -> &'static str {
    self.line_number.name()
  }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub enum Info {
  LineNumber(LineNumber),
  ColumnNumber(ColumnNumber),
  IsStartOfLine(IsStartOfLine),
  IndentLevel(IndentLevel),
  LineStartColumnNumber(LineStartColumnNumber),
  LineStartIndentLevel(LineStartIndentLevel),
}

impl From<LineNumber> for Info {
  fn from(info: LineNumber) -> Self {
    Info::LineNumber(info)
  }
}

impl From<ColumnNumber> for Info {
  fn from(info: ColumnNumber) -> Self {
    Info::ColumnNumber(info)
  }
}

impl From<IsStartOfLine> for Info {
  fn from(info: IsStartOfLine) -> Self {
    Info::IsStartOfLine(info)
  }
}

impl From<IndentLevel> for Info {
  fn from(info: IndentLevel) -> Self {
    Info::IndentLevel(info)
  }
}

impl From<LineStartColumnNumber> for Info {
  fn from(info: LineStartColumnNumber) -> Self {
    Info::LineStartColumnNumber(info)
  }
}

impl From<LineStartIndentLevel> for Info {
  fn from(info: LineStartIndentLevel) -> Self {
    Info::LineStartIndentLevel(info)
  }
}

/// Helper IR that holds line and column number IR.
#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub struct LineAndColumn {
  pub line: LineNumber,
  pub column: ColumnNumber,
}

impl LineAndColumn {
  pub fn new(name: &'static str) -> Self {
    Self {
      line: LineNumber::new(name),
      column: ColumnNumber::new(name),
    }
  }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub struct LineNumber {
  id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl LineNumber {
  pub fn new(_name: &'static str) -> Self {
    Self {
      id: thread_state::next_line_number_id(),
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  #[inline]
  pub fn unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "line_number";
  }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub struct ColumnNumber {
  id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl ColumnNumber {
  pub fn new(_name: &'static str) -> Self {
    Self {
      id: thread_state::next_column_number_id(),
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  #[inline]
  pub fn unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "column_number";
  }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub struct IsStartOfLine {
  id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl IsStartOfLine {
  pub fn new(_name: &'static str) -> Self {
    Self {
      id: thread_state::next_is_start_of_line_id(),
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  #[inline]
  pub fn unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "is_start_of_line";
  }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub struct LineStartColumnNumber {
  id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl LineStartColumnNumber {
  pub fn new(_name: &'static str) -> Self {
    Self {
      id: thread_state::next_line_start_column_number_id(),
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  #[inline]
  pub fn unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "line_start_column_number";
  }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub struct IndentLevel {
  id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl IndentLevel {
  pub fn new(_name: &'static str) -> Self {
    Self {
      id: thread_state::next_indent_level_id(),
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  #[inline]
  pub fn unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "indent_level";
  }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub struct LineStartIndentLevel {
  id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl LineStartIndentLevel {
  pub fn new(_name: &'static str) -> Self {
    Self {
      id: thread_state::next_line_start_indent_level_id(),
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  #[inline]
  pub fn unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "line_start_indent_level";
  }
}

/// Used to re-evaluate a condition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConditionReevaluation {
  pub(crate) condition_reevaluation_id: u32,
  pub(crate) condition_id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl ConditionReevaluation {
  pub(crate) fn new(_name: &'static str, condition_id: u32) -> Self {
    ConditionReevaluation {
      condition_reevaluation_id: thread_state::next_condition_reevaluation_id(),
      condition_id,
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  pub fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "condition_reevaluation";
  }
}

/// Conditionally print items based on a condition.
///
/// These conditions are extremely flexible and can even be resolved based on
/// information found later on in the file.
#[derive(Clone)]
pub struct Condition<'a> {
  /// Unique identifier.
  id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
  /// If a reference has been created for the condition via `create_reference()`. If so, the printer
  /// will store the condition and it will be retrievable via a condition resolver.
  pub(super) is_stored: bool,
  pub(super) store_save_point: bool,
  /// The condition to resolve.
  pub(super) condition: ConditionResolver<'a>,
  /// The items to print when the condition is true.
  pub(super) true_path: Option<PrintItemPath<'a>>,
  /// The items to print when the condition is false or undefined (not yet resolved).
  pub(super) false_path: Option<PrintItemPath<'a>>,
}

impl<'a> Condition<'a> {
  pub fn new(name: &'static str, properties: ConditionProperties<'a>) -> Self {
    Self::new_internal(name, properties)
  }

  pub fn new_true() -> Self {
    Self::new_internal(
      "trueCondition",
      ConditionProperties {
        condition: condition_resolvers::true_resolver(),
        true_path: None,
        false_path: None,
      },
    )
  }

  pub fn new_false() -> Self {
    Self::new_internal(
      "falseCondition",
      ConditionProperties {
        condition: condition_resolvers::false_resolver(),
        true_path: None,
        false_path: None,
      },
    )
  }

  fn new_internal(_name: &'static str, properties: ConditionProperties<'a>) -> Self {
    Self {
      id: thread_state::next_condition_id(),
      is_stored: false,
      store_save_point: false,
      #[cfg(debug_assertions)]
      name: _name,
      condition: properties.condition,
      true_path: properties.true_path.and_then(|x| x.first_node),
      false_path: properties.false_path.and_then(|x| x.first_node),
    }
  }

  #[inline]
  pub fn unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "condition";
  }

  #[inline]
  pub fn true_path(&self) -> &Option<PrintItemPath<'a>> {
    &self.true_path
  }

  #[inline]
  pub fn false_path(&self) -> &Option<PrintItemPath<'a>> {
    &self.false_path
  }

  #[inline]
  pub(super) fn resolve(&self, context: &mut ConditionResolverContext) -> Option<bool> {
    (self.condition)(context)
  }

  pub fn create_reference(&mut self) -> ConditionReference {
    self.is_stored = true;
    ConditionReference::new(self.name(), self.id)
  }

  pub fn create_reevaluation(&mut self) -> ConditionReevaluation {
    self.store_save_point = true;
    self.is_stored = true;
    ConditionReevaluation::new(self.name(), self.id)
  }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug)]
pub struct ConditionReference {
  #[cfg(debug_assertions)]
  pub(super) name: &'static str,
  pub(super) id: u32,
}

impl ConditionReference {
  pub(super) fn new(_name: &'static str, id: u32) -> ConditionReference {
    ConditionReference {
      #[cfg(debug_assertions)]
      name: _name,
      id,
    }
  }

  #[inline]
  pub(super) fn name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "conditionRef";
  }

  /// Creates a condition resolver that checks the value of the condition this references.
  pub fn create_resolver<'a>(&self, arena: &'a Arena) -> ConditionResolver<'a> {
    let captured_self = *self;
    arena.alloc_resolver(move |condition_context| condition_context.resolved_condition(&captured_self))
  }
}

/// Properties for the condition.
pub struct ConditionProperties<'a> {
  /// The condition to resolve.
  pub condition: ConditionResolver<'a>,
  /// The items to print when the condition is true.
  pub true_path: Option<PrintItems<'a>>,
  /// The items to print when the condition is false or undefined (not yet resolved).
  pub false_path: Option<PrintItems<'a>>,
}

/// Function used to resolve a condition, which is allocated in an arena with
/// [`Arena::alloc_resolver`] (or is static, like the ones in `condition_resolvers`).
pub type ConditionResolver<'a> = &'a dyn Fn(&mut ConditionResolverContext) -> Option<bool>;

/// Context used when resolving a condition.
pub struct ConditionResolverContext<'a, 'b> {
  printer: &'a mut Printer<'b>,
  /// Gets the writer info at the condition's location.
  pub writer_info: WriterInfo,
}

impl<'a, 'b> ConditionResolverContext<'a, 'b> {
  pub(super) fn new(printer: &'a mut Printer<'b>, writer_info: WriterInfo) -> Self {
    ConditionResolverContext { printer, writer_info }
  }

  /// Gets if a condition was true, false, or returns None when not yet resolved.
  /// A condition reference can be retrieved by calling the `create_reference()` on a condition.
  pub fn resolved_condition(&mut self, condition_reference: &ConditionReference) -> Option<bool> {
    self.printer.resolved_condition(condition_reference)
  }

  /// Gets a resolved line and column.
  pub fn resolved_line_and_column(&mut self, line_and_column: LineAndColumn) -> Option<(u32, u32)> {
    let line = self.printer.resolved_line_number(line_and_column.line)?;
    let column = self.printer.resolved_column_number(line_and_column.column)?;
    Some((line, column))
  }

  /// Gets the line number or returns None when not yet resolved.
  pub fn resolved_line_number(&mut self, line_number: LineNumber) -> Option<u32> {
    self.printer.resolved_line_number(line_number)
  }

  /// Gets the column number or returns None when not yet resolved.
  pub fn resolved_column_number(&mut self, column_number: ColumnNumber) -> Option<u32> {
    self.printer.resolved_column_number(column_number)
  }

  /// Gets if the info is at the start of the line or returns None when not yet resolved.
  pub fn resolved_is_start_of_line(&mut self, is_start_of_line: IsStartOfLine) -> Option<bool> {
    self.printer.resolved_is_start_of_line(is_start_of_line)
  }

  /// Gets if the indent level at this info or returns None when not yet resolved.
  pub fn resolved_indent_level(&mut self, indent_level: IndentLevel) -> Option<u8> {
    self.printer.resolved_indent_level(indent_level)
  }

  /// Gets the column number at the start of the line this info appears or returns None when not yet resolved.
  pub fn resolved_line_start_column_number(&mut self, line_start_column_number: LineStartColumnNumber) -> Option<u32> {
    self.printer.resolved_line_start_column_number(line_start_column_number)
  }

  /// Gets the indent level at the start of the line this info appears or returns None when not yet resolved.
  pub fn resolved_line_start_indent_level(&mut self, line_start_indent_level: LineStartIndentLevel) -> Option<u8> {
    self.printer.resolved_line_start_indent_level(line_start_indent_level)
  }

  /// Clears the line and column from being stored.
  pub fn clear_line_and_column(&mut self, lc: LineAndColumn) {
    self.clear_info(lc.line);
    self.clear_info(lc.column);
  }

  /// Clears the info from being stored.
  pub fn clear_info(&mut self, info: impl Into<Info>) {
    self.printer.clear_info(info.into())
  }

  /// Moves an already resolved line number by `delta` lines.
  ///
  /// Use this instead of clearing when the text the line number refers to has only moved up or down
  /// the page, so that it stays correct relative to everything around it.
  pub fn shift_line_number(&mut self, info: LineNumber, delta: isize) {
    self.printer.shift_line_number(info, delta)
  }

  /// Gets if the printer is currently forcing no newlines.
  pub fn is_forcing_no_newlines(&self) -> bool {
    self.printer.is_forcing_no_newlines()
  }
}

/// A container that holds the string's value and character count.
#[derive(Clone)]
pub struct StringContainer<'a> {
  /// The string value.
  pub text: &'a str,
  /// The cached character count.
  /// It is much faster to cache this than to recompute it all the time.
  pub(super) char_count: u32,
}

impl<'a> StringContainer<'a> {
  /// Creates a new string container.
  pub fn new(text: &'a str) -> Self {
    let char_count = unicode_width::UnicodeWidthStr::width(text) as u32;
    Self { text, char_count }
  }

  /// This is used by the sc! proc macro and should not be used otherwise
  /// because the character count is pre-computed.
  pub const fn proc_macro_new_with_char_count(text: &'a str, char_count: u32) -> Self {
    Self { text, char_count }
  }
}

/// Information about a certain location being printed.
#[derive(Clone, Debug)]
pub struct WriterInfo {
  pub line_number: u32,
  pub column_number: u32,
  pub indent_level: u8,
  pub line_start_indent_level: u8,
  /// The columns of spaces after the indentation that the line started with.
  pub line_start_align_columns: u32,
  pub indent_width: u8,
  pub expect_newline_next: bool,
}

impl WriterInfo {
  /// Gets if the current column number equals the line start column number
  /// or if a newline is expected next.
  pub fn is_start_of_line(&self) -> bool {
    self.expect_newline_next || self.is_column_number_at_line_start()
  }

  /// Gets if the start of the line is indented.
  pub fn is_start_of_line_indented(&self) -> bool {
    self.line_start_indent_level > self.indent_level
  }

  /// Gets if the current column number is at the line start column number.
  pub fn is_column_number_at_line_start(&self) -> bool {
    self.column_number == self.line_start_column_number()
  }

  pub fn line_start_column_number(&self) -> u32 {
    (self.line_start_indent_level as u32) * (self.indent_width as u32) + self.line_start_align_columns
  }

  /// Gets the line and column number.
  pub fn line_and_column(&self) -> (u32, u32) {
    (self.line_number, self.column_number)
  }
}

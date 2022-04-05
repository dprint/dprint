use std::cell::UnsafeCell;
use std::mem;
use std::rc::Rc;

use super::condition_resolvers;
use super::printer::Printer;
use super::thread_state;

/** Print Items */

#[derive(Default)]
pub struct PrintItems {
  pub(super) first_node: Option<PrintItemPath>,
  last_node: Option<PrintItemPath>,
}

impl PrintItems {
  pub fn new() -> Self {
    Self {
      first_node: None,
      last_node: None,
    }
  }

  pub fn into_rc_path(self) -> Option<PrintItemPath> {
    self.first_node
  }

  pub fn push_item(&mut self, item: PrintItem) {
    self.push_item_internal(item);
  }

  #[inline]
  fn push_item_internal(&mut self, item: PrintItem) {
    let node = thread_state::with_bump_allocator(|bump| {
      let result = bump.alloc(PrintNodeCell::new(item));
      unsafe { std::mem::transmute::<&PrintNodeCell, UnsafePrintLifetime<PrintNodeCell>>(result) }
    });
    if let Some(first_node) = &self.first_node {
      let new_last_node = node.get_last_next().unwrap_or(node);
      self.last_node.as_ref().unwrap_or(first_node).set_next(Some(node));
      self.last_node = Some(new_last_node);
    } else {
      self.last_node = node.get_last_next();
      self.first_node = Some(node);
    }
  }
}

impl PrintItems {
  pub fn extend(&mut self, items: PrintItems) {
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

  pub fn push_str(&mut self, item: &str) {
    self.push_string(item.to_string());
  }

  pub fn push_string(&mut self, item: String) {
    let string_container = thread_state::with_bump_allocator(|bump| {
      let result = bump.alloc(StringContainer::new(item));
      unsafe { std::mem::transmute::<&StringContainer, UnsafePrintLifetime<StringContainer>>(result) }
    });
    self.push_item_internal(PrintItem::String(string_container));
  }

  pub fn push_condition(&mut self, condition: Condition) {
    let condition = thread_state::with_bump_allocator(|bump| {
      let result = bump.alloc(condition);
      unsafe { std::mem::transmute::<&Condition, UnsafePrintLifetime<Condition>>(result) }
    });
    self.push_item_internal(PrintItem::Condition(condition));
  }

  pub fn push_line_and_column(&mut self, line_and_col: LineAndColumn) {
    self.push_line_number(line_and_col.line);
    self.push_column_number(line_and_col.column);
  }

  pub fn push_line_number(&mut self, line_number: LineNumber) {
    self.push_item_internal(PrintItem::Info(Info::LineNumber(line_number)));
  }

  pub fn push_column_number(&mut self, column_number: ColumnNumber) {
    self.push_item_internal(PrintItem::Info(Info::ColumnNumber(column_number)));
  }

  pub fn push_is_start_of_line(&mut self, is_start_of_line: IsStartOfLine) {
    self.push_item_internal(PrintItem::Info(Info::IsStartOfLine(is_start_of_line)));
  }

  pub fn push_indent_level(&mut self, indent_level: IndentLevel) {
    self.push_item_internal(PrintItem::Info(Info::IndentLevel(indent_level)));
  }

  pub fn push_line_start_column_number(&mut self, line_start_column_number: LineStartColumnNumber) {
    self.push_item_internal(PrintItem::Info(Info::LineStartColumnNumber(line_start_column_number)));
  }

  pub fn push_line_start_indent_level(&mut self, line_start_indent_level: LineStartIndentLevel) {
    self.push_item_internal(PrintItem::Info(Info::LineStartIndentLevel(line_start_indent_level)));
  }

  pub fn push_line_number_anchor(&mut self, anchor: LineNumberAnchor) {
    self.push_item_internal(PrintItem::Anchor(Anchor::LineNumber(anchor)));
  }

  pub fn push_reevaluation(&mut self, condition_reevaluation: ConditionReevaluation) {
    self.push_item_internal(PrintItem::ConditionReevaluation(condition_reevaluation));
  }

  pub fn push_signal(&mut self, signal: Signal) {
    self.push_item_internal(PrintItem::Signal(signal));
  }

  pub fn push_path(&mut self, path: PrintItemPath) {
    self.push_item_internal(PrintItem::RcPath(path))
  }

  pub fn push_optional_path(&mut self, path: Option<PrintItemPath>) {
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
              text.push_str(&get_items_as_text(true_path, format!("{}    ", &indent_text)));
            }
            if let Some(false_path) = &condition.false_path {
              text.push_str(&get_line(String::from("  false:"), &indent_text));
              text.push_str(&get_items_as_text(false_path, format!("{}    ", &indent_text)));
            }
          }
          PrintItem::String(str_text) => text.push_str(&get_line(format!("`{}`", str_text.text), &indent_text)),
          PrintItem::RcPath(path) => text.push_str(&get_items_as_text(path, indent_text.clone())),
          PrintItem::Anchor(Anchor::LineNumber(line_number_anchor)) => {
            text.push_str(&get_line(format!("Line number anchor: {}", line_number_anchor.get_name()), &indent_text))
          }
          PrintItem::Info(info) => {
            let (desc, name) = match info {
              Info::LineNumber(info) => ("Line number", info.get_name()),
              Info::ColumnNumber(info) => ("Column number", info.get_name()),
              Info::IsStartOfLine(info) => ("Is start of line", info.get_name()),
              Info::IndentLevel(info) => ("Indent level", info.get_name()),
              Info::LineStartColumnNumber(info) => ("Line start column number", info.get_name()),
              Info::LineStartIndentLevel(info) => ("Line start indent level", info.get_name()),
            };
            text.push_str(&get_line(format!("{}: {}", desc, name), &indent_text))
          }
          PrintItem::ConditionReevaluation(reevaluation) => {
            text.push_str(&get_line(format!("Condition reevaluation: {}", reevaluation.get_name()), &indent_text))
          }
        }
      }

      return text;

      fn get_line(text: String, indent_text: &str) -> String {
        format!("{}{}\n", indent_text, text)
      }
    }
  }

  pub fn iter(&self) -> PrintItemsIterator {
    PrintItemsIterator { node: self.first_node }
  }
}

pub struct PrintItemsIterator {
  node: Option<PrintItemPath>,
}

impl PrintItemsIterator {
  pub fn new(path: PrintItemPath) -> Self {
    Self { node: Some(path) }
  }
}

impl Iterator for PrintItemsIterator {
  type Item = PrintItem;

  fn next(&mut self) -> Option<PrintItem> {
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

impl From<&'static str> for PrintItems {
  fn from(value: &'static str) -> Self {
    let mut items = PrintItems::new();
    items.push_str(value);
    items
  }
}

impl From<String> for PrintItems {
  fn from(value: String) -> Self {
    let mut items = PrintItems::new();
    items.push_string(value);
    items
  }
}

impl From<Condition> for PrintItems {
  fn from(value: Condition) -> Self {
    let mut items = PrintItems::new();
    items.push_condition(value);
    items
  }
}

impl From<Signal> for PrintItems {
  fn from(value: Signal) -> Self {
    let mut items = PrintItems::new();
    items.push_signal(value);
    items
  }
}

impl From<PrintItemPath> for PrintItems {
  fn from(value: PrintItemPath) -> Self {
    let mut items = PrintItems::new();
    items.push_path(value);
    items
  }
}

impl<T> From<Option<T>> for PrintItems
where
  PrintItems: From<T>,
{
  fn from(value: Option<T>) -> Self {
    value.map(PrintItems::from).unwrap_or_default()
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
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceInfo {
  pub info_id: u32,
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

/** Print Node */

pub struct PrintNode {
  pub(super) next: Option<PrintItemPath>,
  pub(super) item: PrintItem,
  #[cfg(feature = "tracing")]
  pub print_node_id: u32,
}

impl PrintNode {
  fn new(item: PrintItem) -> PrintNode {
    PrintNode {
      item,
      next: None,
      #[cfg(feature = "tracing")]
      print_node_id: thread_state::next_print_node_id(),
    }
  }

  fn set_next(&mut self, new_next: Option<PrintItemPath>) {
    let past_next = mem::replace(&mut self.next, new_next);

    if let Some(past_next) = past_next {
      if let Some(new_next) = new_next {
        new_next.get_last_next().unwrap_or(new_next).set_next(Some(past_next));
      }
    }
  }
}

/// A fast implementation of RefCell<PrintNode> that avoids runtime checks on borrows.
pub struct PrintNodeCell {
  value: UnsafeCell<PrintNode>,
}

impl PrintNodeCell {
  pub(super) fn new(item: PrintItem) -> PrintNodeCell {
    PrintNodeCell {
      value: UnsafeCell::new(PrintNode::new(item)),
    }
  }

  #[inline]
  pub(super) fn get_item(&self) -> PrintItem {
    unsafe { (*self.value.get()).item.clone() }
  }

  #[inline]
  pub(super) fn get_next(&self) -> Option<PrintItemPath> {
    unsafe { (*self.value.get()).next }
  }

  #[inline]
  pub(super) fn set_next(&self, new_next: Option<PrintItemPath>) {
    unsafe {
      (*self.value.get()).set_next(new_next);
    }
  }

  #[inline]
  pub(super) fn get_last_next(&self) -> Option<PrintItemPath> {
    let mut current = self.get_next();
    loop {
      if let Some(last) = &current {
        if let Some(next) = last.get_next() {
          current.replace(next);
          continue;
        }
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
  pub(super) unsafe fn get_node(&self) -> *mut PrintNode {
    self.value.get()
  }

  #[inline]
  pub fn take_next(self) -> Option<PrintItemPath> {
    self.value.into_inner().next.take()
  }
}

pub type PrintItemPath = UnsafePrintLifetime<PrintNodeCell>;

/// This lifetime value is a lie that is not represented or enforced by the compiler.
/// What actually happens is the reference will remain active until the print
/// items are printed. At that point, it's unsafe to use them anymore.
///
/// To get around this unsafeness, the API would have to be sacrificed by passing
/// around an object that wraps an arena. Perhaps that will be the way going forward
/// in the future, but for now this was an easy way to get the big performance
/// boost from an arena without changing the API much.
type UnsafePrintLifetime<T> = &'static T;

/* Print item and kinds */

/// The different items the printer could encounter.
#[derive(Clone)]
pub enum PrintItem {
  String(UnsafePrintLifetime<StringContainer>),
  Condition(UnsafePrintLifetime<Condition>),
  Signal(Signal),
  RcPath(PrintItemPath),
  Anchor(Anchor),
  Info(Info),
  ConditionReevaluation(ConditionReevaluation),
}

#[derive(Clone, PartialEq, Copy, Debug, serde::Serialize)]
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

/// Handles updating the position of a future resolved line
/// number if the anchor changes.
#[derive(Clone)]
pub struct LineNumberAnchor {
  id: u32,
  line_number: LineNumber,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl LineNumberAnchor {
  pub fn new(line_number: LineNumber) -> Self {
    Self {
      id: thread_state::next_line_number_anchor_id(),
      line_number,
      #[cfg(debug_assertions)]
      name: line_number.name,
    }
  }

  #[inline]
  pub fn get_unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn get_line_number_id(&self) -> u32 {
    self.line_number.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "line_number_anchor";
  }
}

#[derive(Clone, PartialEq, Copy, Debug)]
pub enum Info {
  LineNumber(LineNumber),
  ColumnNumber(ColumnNumber),
  IsStartOfLine(IsStartOfLine),
  IndentLevel(IndentLevel),
  LineStartColumnNumber(LineStartColumnNumber),
  LineStartIndentLevel(LineStartIndentLevel),
}

/// Helper IR that holds line and column number IR.
#[derive(Clone, PartialEq, Copy, Debug)]
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

#[derive(Clone, PartialEq, Copy, Debug)]
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
  pub fn get_unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "line_number";
  }
}

#[derive(Clone, PartialEq, Copy, Debug)]
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
  pub fn get_unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "column_number";
  }
}

#[derive(Clone, PartialEq, Copy, Debug)]
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
  pub fn get_unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "is_start_of_line";
  }
}

#[derive(Clone, PartialEq, Copy, Debug)]
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
  pub fn get_unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "line_start_column_number";
  }
}

#[derive(Clone, PartialEq, Copy, Debug)]
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
  pub fn get_unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "indent_level";
  }
}

#[derive(Clone, PartialEq, Copy, Debug)]
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
  pub fn get_unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "line_start_indent_level";
  }
}

/// Used to re-evaluate a condition.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ConditionReevaluation {
  pub(crate) condition_id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

impl ConditionReevaluation {
  pub(crate) fn new(_name: &'static str, condition_id: u32) -> Self {
    ConditionReevaluation {
      condition_id: condition_id,
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  pub fn get_name(&self) -> &'static str {
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
pub struct Condition {
  /// Unique identifier.
  id: u32,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
  /// If a reference has been created for the condition via `get_reference()`. If so, the printer
  /// will store the condition and it will be retrievable via a condition resolver.
  pub(super) is_stored: bool,
  pub(super) store_save_point: bool,
  /// The condition to resolve.
  pub(super) condition: ConditionResolver,
  /// The items to print when the condition is true.
  pub(super) true_path: Option<PrintItemPath>,
  /// The items to print when the condition is false or undefined (not yet resolved).
  pub(super) false_path: Option<PrintItemPath>,
}

impl Condition {
  pub fn new(name: &'static str, properties: ConditionProperties) -> Self {
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

  fn new_internal(_name: &'static str, properties: ConditionProperties) -> Self {
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
  pub fn get_unique_id(&self) -> u32 {
    self.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "condition";
  }

  #[inline]
  pub fn get_true_path(&self) -> &Option<PrintItemPath> {
    &self.true_path
  }

  #[inline]
  pub fn get_false_path(&self) -> &Option<PrintItemPath> {
    &self.false_path
  }

  #[inline]
  pub(super) fn resolve(&self, context: &mut ConditionResolverContext) -> Option<bool> {
    (self.condition)(context)
  }

  pub fn get_reference(&mut self) -> ConditionReference {
    self.is_stored = true;
    ConditionReference::new(self.get_name(), self.id)
  }

  pub fn create_reevaluation(&mut self) -> ConditionReevaluation {
    self.store_save_point = true;
    self.is_stored = true;
    ConditionReevaluation::new(self.get_name(), self.id)
  }
}

#[derive(Clone, PartialEq, Copy, Debug)]
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
  pub(super) fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "conditionRef";
  }

  /// Creates a condition resolver that checks the value of the condition this references.
  pub fn create_resolver(&self) -> ConditionResolver {
    let captured_self = *self;
    Rc::new(move |condition_context: &mut ConditionResolverContext| condition_context.get_resolved_condition(&captured_self))
  }
}

/// Properties for the condition.
pub struct ConditionProperties {
  /// The condition to resolve.
  pub condition: ConditionResolver,
  /// The items to print when the condition is true.
  pub true_path: Option<PrintItems>,
  /// The items to print when the condition is false or undefined (not yet resolved).
  pub false_path: Option<PrintItems>,
}

/// Function used to resolve a condition.
pub type ConditionResolver = Rc<dyn Fn(&mut ConditionResolverContext) -> Option<bool>>;

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
  /// A condition reference can be retrieved by calling the `get_reference()` on a condition.
  pub fn get_resolved_condition(&mut self, condition_reference: &ConditionReference) -> Option<bool> {
    self.printer.get_resolved_condition(condition_reference)
  }

  /// Gets a resolved line and column.
  pub fn get_resolved_line_and_column(&self, line_and_column: LineAndColumn) -> Option<(u32, u32)> {
    let line = self.printer.get_resolved_line_number(line_and_column.line)?;
    let column = self.printer.get_resolved_column_number(line_and_column.column)?;
    Some((line, column))
  }

  /// Gets the line number or returns None when not yet resolved.
  pub fn get_resolved_line_number(&self, line_number: LineNumber) -> Option<u32> {
    self.printer.get_resolved_line_number(line_number)
  }

  /// Gets the column number or returns None when not yet resolved.
  pub fn get_resolved_column_number(&self, column_number: ColumnNumber) -> Option<u32> {
    self.printer.get_resolved_column_number(column_number)
  }

  /// Gets if the info is at the start of the line or returns None when not yet resolved.
  pub fn get_resolved_is_start_of_line(&self, is_start_of_line: IsStartOfLine) -> Option<bool> {
    self.printer.get_resolved_is_start_of_line(is_start_of_line)
  }

  /// Gets if the indent level at this info or returns None when not yet resolved.
  pub fn get_resolved_indent_level(&self, indent_level: IndentLevel) -> Option<u8> {
    self.printer.get_resolved_indent_level(indent_level)
  }

  /// Gets the column number at the start of the line this info appears or returns None when not yet resolved.
  pub fn get_resolved_line_start_column_number(&self, line_start_column_number: LineStartColumnNumber) -> Option<u32> {
    self.printer.get_resolved_line_start_column_number(line_start_column_number)
  }

  /// Gets the indent level at the start of the line this info appears or returns None when not yet resolved.
  pub fn get_resolved_line_start_indent_level(&self, line_start_indent_level: LineStartIndentLevel) -> Option<u8> {
    self.printer.get_resolved_line_start_indent_level(line_start_indent_level)
  }

  /// Clears the line number from being stored.
  pub fn clear_line_number(&mut self, line_number: LineNumber) {
    self.printer.clear_line_number(line_number)
  }

  /// Clears the column number from being stored.
  pub fn clear_column_number(&mut self, column_number: ColumnNumber) {
    self.printer.clear_column_number(column_number)
  }

  /// Clears the info from being stored.
  pub fn clear_is_start_of_line(&mut self, is_start_of_line: IsStartOfLine) {
    self.printer.clear_is_start_of_line(is_start_of_line)
  }

  /// Clears the info from being stored.
  pub fn clear_line_start_column_number(&mut self, col_number: LineStartColumnNumber) {
    self.printer.clear_line_start_column_number(col_number)
  }

  /// Clears the info from being stored.
  pub fn clear_line_start_indent_level(&mut self, line_start_indent_level: LineStartIndentLevel) {
    self.printer.clear_line_start_indent_level(line_start_indent_level)
  }
}

/// A container that holds the string's value and character count.
#[derive(Clone)]
pub struct StringContainer {
  /// The string value.
  pub text: String,
  /// The cached character count.
  /// It is much faster to cache this than to recompute it all the time.
  pub(super) char_count: u32,
}

impl StringContainer {
  /// Creates a new string container.
  pub fn new(text: String) -> Self {
    let char_count = text.chars().count() as u32;
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
    (self.line_start_indent_level as u32) * (self.indent_width as u32)
  }

  /// Gets the line and column number.
  pub fn get_line_and_column(&self) -> (u32, u32) {
    (self.line_number, self.column_number)
  }
}

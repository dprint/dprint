use std::cell::UnsafeCell;
use std::mem;
use std::rc::Rc;

use super::printer::Printer;
use super::utils::with_bump_allocator;
use super::utils::CounterCell;

/** Print Items */

#[derive(Default)]
pub struct PrintItems {
  pub(super) first_node: Option<PrintItemPath>,
  last_node: Option<PrintItemPath>,
}

impl PrintItems {
  pub fn new() -> PrintItems {
    PrintItems {
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
    let node = with_bump_allocator(|bump| {
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
    let string_container = with_bump_allocator(|bump| {
      let result = bump.alloc(StringContainer::new(item));
      unsafe { std::mem::transmute::<&StringContainer, UnsafePrintLifetime<StringContainer>>(result) }
    });
    self.push_item_internal(PrintItem::String(string_container));
  }

  pub fn push_condition(&mut self, condition: Condition) {
    let condition = with_bump_allocator(|bump| {
      let result = bump.alloc(condition);
      unsafe { std::mem::transmute::<&Condition, UnsafePrintLifetime<Condition>>(result) }
    });
    self.push_item_internal(PrintItem::Condition(condition));
  }

  pub fn push_info(&mut self, info: Info) {
    self.push_item_internal(PrintItem::Info(info));
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
      get_items_as_text(first_node.clone(), String::from(""))
    } else {
      String::new()
    };

    fn get_items_as_text(items: PrintItemPath, indent_text: String) -> String {
      let mut text = String::new();
      for item in PrintItemsIterator::new(items) {
        match item {
          PrintItem::Signal(signal) => text.push_str(&get_line(format!("Signal::{:?}", signal), &indent_text)),
          PrintItem::Info(info) => text.push_str(&get_line(format!("Info: {}", info.name), &indent_text)),
          PrintItem::Condition(condition) => {
            text.push_str(&get_line(format!("Condition: {}", condition.name), &indent_text));
            if let Some(true_path) = &condition.true_path {
              text.push_str(&get_line(String::from("  true:"), &indent_text));
              text.push_str(&get_items_as_text(true_path.clone(), format!("{}    ", &indent_text)));
            }
            if let Some(false_path) = &condition.false_path {
              text.push_str(&get_line(String::from("  false:"), &indent_text));
              text.push_str(&get_items_as_text(false_path.clone(), format!("{}    ", &indent_text)));
            }
          }
          PrintItem::String(str_text) => text.push_str(&get_line(format!("`{}`", str_text.text.to_string()), &indent_text)),
          PrintItem::RcPath(path) => text.push_str(&get_items_as_text(path.clone(), indent_text.clone())),
        }
      }

      return text;

      fn get_line(text: String, indent_text: &str) -> String {
        format!("{}{}\n", indent_text, text)
      }
    }
  }

  pub fn iter(&self) -> PrintItemsIterator {
    PrintItemsIterator { node: self.first_node.clone() }
  }
}

pub struct PrintItemsIterator {
  node: Option<PrintItemPath>,
}

impl PrintItemsIterator {
  pub fn new(path: PrintItemPath) -> PrintItemsIterator {
    PrintItemsIterator { node: Some(path) }
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
  pub print_node_id: usize,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub writer_node_id: Option<usize>,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceWriterNode {
  pub writer_node_id: usize,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub previous_node_id: Option<usize>,
  pub text: String,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TracePrintNode {
  pub print_node_id: usize,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub next_print_node_id: Option<usize>,
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
  RcPath(usize),
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceInfo {
  pub info_id: usize,
  pub name: String,
}

#[cfg(feature = "tracing")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceCondition {
  pub condition_id: usize,
  pub name: String,
  pub is_stored: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  /// Identifier to the true path print node.
  pub true_path: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  /// Identifier to the false path print node.
  pub false_path: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  /// Any infos that should cause the re-evaluation of this condition.
  /// This is only done on request for performance reasons.
  pub dependent_infos: Option<Vec<usize>>,
}

/** Print Node */

pub struct PrintNode {
  pub(super) next: Option<PrintItemPath>,
  pub(super) item: PrintItem,
  #[cfg(feature = "tracing")]
  pub print_node_id: usize,
}

#[cfg(feature = "tracing")]
thread_local! {
    static PRINT_NODE_COUNTER: CounterCell = CounterCell::new();
}

impl PrintNode {
  fn new(item: PrintItem) -> PrintNode {
    PrintNode {
      item,
      next: None,
      #[cfg(feature = "tracing")]
      print_node_id: PRINT_NODE_COUNTER.with(|counter| counter.increment()),
    }
  }

  fn set_next(&mut self, new_next: Option<PrintItemPath>) {
    let past_next = mem::replace(&mut self.next, new_next.clone());

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
    unsafe { (*self.value.get()).next.clone() }
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
  pub(super) fn get_node_id(&self) -> usize {
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
  Info(Info),
  Signal(Signal),
  RcPath(PrintItemPath),
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

/// Can be used to get information at a certain location being printed. These
/// can be resolved by providing the info object to a condition context's
/// get_resolved_info(&info) method.
#[derive(Clone, PartialEq, Copy, Debug)]
pub struct Info {
  /// Unique identifier.
  id: usize,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
}

thread_local! {
    static INFO_COUNTER: CounterCell = CounterCell::new();
}

impl Info {
  pub fn new(_name: &'static str) -> Info {
    Info {
      id: INFO_COUNTER.with(|counter| counter.increment()),
      #[cfg(debug_assertions)]
      name: _name,
    }
  }

  #[inline]
  pub fn get_unique_id(&self) -> usize {
    self.id
  }

  #[inline]
  pub fn get_name(&self) -> &'static str {
    #[cfg(debug_assertions)]
    return self.name;
    #[cfg(not(debug_assertions))]
    return "info";
  }
}

/// Conditionally print items based on a condition.
///
/// These conditions are extremely flexible and can even be resolved based on
/// information found later on in the file.
#[derive(Clone)]
pub struct Condition {
  /// Unique identifier.
  id: usize,
  /// Name for debugging purposes.
  #[cfg(debug_assertions)]
  name: &'static str,
  /// If a reference has been created for the condition via `get_reference()`. If so, the printer
  /// will store the condition and it will be retrievable via a condition resolver.
  pub(super) is_stored: bool,
  /// The condition to resolve.
  pub(super) condition: Rc<ConditionResolver>,
  /// The items to print when the condition is true.
  pub(super) true_path: Option<PrintItemPath>,
  /// The items to print when the condition is false or undefined (not yet resolved).
  pub(super) false_path: Option<PrintItemPath>,
  /// Any infos that should cause the re-evaluation of this condition.
  /// This is only done on request for performance reasons.
  pub(super) dependent_infos: Option<Vec<Info>>,
}

thread_local! {
    static CONDITION_COUNTER: CounterCell = CounterCell::new();
}

impl Condition {
  pub fn new(name: &'static str, properties: ConditionProperties) -> Condition {
    Condition::new_internal(name, properties, None)
  }

  pub fn new_true() -> Condition {
    Condition::new_internal(
      "trueCondition",
      ConditionProperties {
        condition: Rc::new(|_| Some(true)),
        true_path: None,
        false_path: None,
      },
      None,
    )
  }

  pub fn new_false() -> Condition {
    Condition::new_internal(
      "falseCondition",
      ConditionProperties {
        condition: Rc::new(|_| Some(false)),
        true_path: None,
        false_path: None,
      },
      None,
    )
  }

  pub fn new_with_dependent_infos(name: &'static str, properties: ConditionProperties, dependent_infos: Vec<Info>) -> Condition {
    Condition::new_internal(name, properties, Some(dependent_infos))
  }

  fn new_internal(_name: &'static str, properties: ConditionProperties, dependent_infos: Option<Vec<Info>>) -> Condition {
    Condition {
      id: CONDITION_COUNTER.with(|counter| counter.increment()),
      is_stored: dependent_infos.is_some(),
      #[cfg(debug_assertions)]
      name: _name,
      condition: properties.condition,
      true_path: properties.true_path.map(|x| x.first_node).flatten(),
      false_path: properties.false_path.map(|x| x.first_node).flatten(),
      dependent_infos,
    }
  }

  #[inline]
  pub fn get_unique_id(&self) -> usize {
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
}

#[derive(Clone, PartialEq, Copy, Debug)]
pub struct ConditionReference {
  #[cfg(debug_assertions)]
  pub(super) name: &'static str,
  pub(super) id: usize,
}

impl ConditionReference {
  pub(super) fn new(_name: &'static str, id: usize) -> ConditionReference {
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
  pub fn create_resolver(&self) -> impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static {
    let captured_self = self.clone();
    move |condition_context: &mut ConditionResolverContext| condition_context.get_resolved_condition(&captured_self)
  }
}

/// Properties for the condition.
pub struct ConditionProperties {
  /// The condition to resolve.
  pub condition: Rc<ConditionResolver>,
  /// The items to print when the condition is true.
  pub true_path: Option<PrintItems>,
  /// The items to print when the condition is false or undefined (not yet resolved).
  pub false_path: Option<PrintItems>,
}

/// Function used to resolve a condition.
pub type ConditionResolver = dyn Fn(&mut ConditionResolverContext) -> Option<bool>;

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

  /// Gets if a condition was true, false, or returns undefined when not yet resolved.
  /// A condition reference can be retrieved by calling the `get_reference()` on a condition.
  pub fn get_resolved_condition(&mut self, condition_reference: &ConditionReference) -> Option<bool> {
    self.printer.get_resolved_condition(condition_reference)
  }

  /// Gets the writer info at a specified info or returns undefined when not yet resolved.
  pub fn get_resolved_info(&self, info: &Info) -> Option<&WriterInfo> {
    self.printer.get_resolved_info(info)
  }

  /// Clears the info result from being stored.
  pub fn clear_info(&mut self, info: &Info) {
    self.printer.clear_info(info)
  }

  /// Gets if the provided info has moved positions since the last check.
  /// Returns None when the info can't be resolved. Returns Some(false) the first time this is called.
  pub fn has_info_moved(&mut self, info: &Info) -> Option<bool> {
    self.printer.has_info_moved(info)
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
  pub fn new(text: String) -> StringContainer {
    let char_count = text.chars().count() as u32;
    StringContainer { text, char_count }
  }
}

/// Information about a certain location being printed.
#[derive(Clone, Debug)]
pub struct WriterInfo {
  pub line_number: u32,
  pub column_number: u32,
  pub indent_level: u8,
  pub line_start_indent_level: u8,
  pub line_start_column_number: u32,
}

impl WriterInfo {
  /// Gets if the current column number equals the line start column number.
  pub fn is_start_of_line(&self) -> bool {
    self.column_number == self.line_start_column_number
  }

  /// Gets the line and column number.
  pub fn get_line_and_column(&self) -> (u32, u32) {
    (self.line_number, self.column_number)
  }
}

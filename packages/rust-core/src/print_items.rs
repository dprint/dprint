use super::printer::Printer;
use std::rc::Rc;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::mem;

/* Traits -- This allows implementing these for Wasm objects. */

pub trait StringTrait {
    fn get_length(&self) -> usize;
    fn get_text<'a>(&'a self) -> &'a str;
}

impl StringTrait for String {
    fn get_length(&self) -> usize {
        self.chars().count()
    }

    fn get_text<'a>(&'a self) -> &'a str {
        self
    }
}

pub trait InfoTrait {
    fn get_unique_id(&self) -> usize;
    fn get_name(&self) -> &'static str;
}

pub trait ConditionTrait<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    fn get_unique_id(&self) -> usize;
    fn get_is_stored(&self) -> bool;
    fn get_name(&self) -> &'static str;
    fn resolve(&self, context: &mut ConditionResolverContext<TString, TInfo, TCondition>) -> Option<bool>;
    fn get_true_path(&self) -> Option<PrintItemPath<TString, TInfo, TCondition>>;
    fn get_false_path(&self) -> Option<PrintItemPath<TString, TInfo, TCondition>>;
    fn get_dependent_infos<'a>(&'a self) -> &'a Option<Vec<TInfo>>;
}

/** Print Items */

pub struct PrintItems<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    pub(super) first_node: Option<PrintItemPath<TString, TInfo, TCondition>>,
    last_node: Option<PrintItemPath<TString, TInfo, TCondition>>,
}

impl<TString, TInfo, TCondition> PrintItems<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    pub fn new() -> PrintItems<TString, TInfo, TCondition> {
        PrintItems {
            first_node: None,
            last_node: None,
        }
    }

    pub fn into_rc_path(self) -> Option<PrintItemPath<TString, TInfo, TCondition>> {
        self.first_node
    }

    pub fn push_item(&mut self, item: PrintItem<TString, TInfo, TCondition>) {
        self.push_item_internal(item);
    }

    // seems marginally faster to inline this? probably not worth it
    #[inline]
    fn push_item_internal(&mut self, item: PrintItem<TString, TInfo, TCondition>) {
        let node = Rc::new(PrintNodeCell::new(item));
        if let Some(first_node) = &self.first_node {
            let new_last_node = node.get_last_next().unwrap_or(node.clone());
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
        if let Some(first_node) = &self.first_node {
            self.last_node.as_ref().unwrap_or(first_node).set_next(items.first_node.clone());
            self.last_node = items.last_node.or(items.first_node.or(self.last_node.clone())); // todo: fix this
        } else {
            self.first_node = items.first_node;
            self.last_node = items.last_node;
        }
    }

    pub fn push_str(&mut self, item: &str) {
        self.push_item_internal(PrintItem::String(Rc::from(StringContainer::new(String::from(item)))));
    }

    pub fn push_condition(&mut self, condition: Condition) {
        self.push_item_internal(PrintItem::Condition(Rc::from(condition)));
    }

    pub fn push_info(&mut self, info: Info) {
        self.push_item_internal(PrintItem::Info(Rc::from(info)));
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
                        if let Some(true_path) = condition.get_true_path() {
                            text.push_str(&get_line(String::from("  true:"), &indent_text));
                            text.push_str(&get_items_as_text(true_path.clone(), format!("{}    ", &indent_text)));
                        }
                        if let Some(false_path) = condition.get_false_path() {
                            text.push_str(&get_line(String::from("  false:"), &indent_text));
                            text.push_str(&get_items_as_text(false_path.clone(), format!("{}    ", &indent_text)));
                        }
                    },
                    PrintItem::String(str_text) => text.push_str(&get_line(format!("`{}`", str_text.text.to_string()), &indent_text)),
                    PrintItem::RcPath(path) => text.push_str(&get_items_as_text(path.clone(), indent_text.clone())),
                }
            }

            return text;

            fn get_line(text: String, indent_text: &String) -> String {
                format!("{}{}\n", indent_text, text)
            }
        }
    }

    pub fn iter(&self) -> PrintItemsIterator {
        PrintItemsIterator {
            node: self.first_node.clone(),
        }
    }
}

pub struct PrintItemsIterator {
    node: Option<PrintItemPath>,
}

impl PrintItemsIterator {
    pub fn new(path: PrintItemPath) -> PrintItemsIterator {
        PrintItemsIterator {
            node: Some(path),
        }
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
            },
            None => None
        }
    }
}

impl Into<PrintItems> for &str {
    fn into(self) -> PrintItems {
        let mut items = PrintItems::new();
        items.push_str(self);
        items
    }
}

impl Into<PrintItems> for String {
    fn into(self) -> PrintItems {
        let mut items = PrintItems::new();
        items.push_str(&self);
        items
    }
}

impl Into<PrintItems> for &String {
    fn into(self) -> PrintItems {
        let mut items = PrintItems::new();
        items.push_str(self);
        items
    }
}

impl Into<PrintItems> for Condition {
    fn into(self) -> PrintItems {
        let mut items = PrintItems::new();
        items.push_condition(self);
        items
    }
}

impl Into<PrintItems> for Signal {
    fn into(self) -> PrintItems {
        let mut items = PrintItems::new();
        items.push_signal(self);
        items
    }
}

impl Into<PrintItems> for Option<PrintItemPath> {
    fn into(self) -> PrintItems {
        let mut items = PrintItems::new();
        if let Some(path) = self {
            items.push_path(path);
        }
        items
    }
}

/** Print Node */

pub struct PrintNode<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    pub(super) next: Option<PrintItemPath<TString, TInfo, TCondition>>,
    pub(super) item: PrintItem<TString, TInfo, TCondition>,
}

impl<TString, TInfo, TCondition> Drop for PrintNode<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    // Implement a custom drop in order to prevent a stack overflow error when dropping objects of this type
    fn drop(&mut self) {
        let mut next = mem::replace(&mut self.next, None);

        loop {
            next = match next {
                Some(node) => match Rc::try_unwrap(node) {
                    Ok(node) => node.take_next(),
                    Err(_) => break,
                },
                None => break
            }
        }
    }
}

impl<TString, TInfo, TCondition> PrintNode<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    fn new(item: PrintItem<TString, TInfo, TCondition>) -> PrintNode<TString, TInfo, TCondition> {
        PrintNode {
            item,
            next: None,
        }
    }

    fn set_next(&mut self, new_next: Option<PrintItemPath<TString, TInfo, TCondition>>) {
        let past_next = mem::replace(&mut self.next, new_next.clone());

        if let Some(past_next) = past_next {
            if let Some(new_next) = new_next {
                new_next.get_last_next().unwrap_or(new_next).set_next(Some(past_next));
            }
        }
    }
}

/// A fast implementation of RefCell<PrintNode> that avoids runtime checks on borrows.
pub struct PrintNodeCell<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    value: UnsafeCell<PrintNode<TString, TInfo, TCondition>>,
}

impl<TString, TInfo, TCondition> PrintNodeCell<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    pub(super) fn new(item: PrintItem<TString, TInfo, TCondition>) -> PrintNodeCell<TString, TInfo, TCondition> {
        PrintNodeCell {
            value: UnsafeCell::new(PrintNode::new(item))
        }
    }

    #[inline]
    pub(super) fn get_item(&self) -> PrintItem<TString, TInfo, TCondition> {
        unsafe {
            (*self.value.get()).item.clone()
        }
    }

    #[inline]
    pub(super) fn get_next(&self) -> Option<PrintItemPath<TString, TInfo, TCondition>> {
        unsafe {
            (*self.value.get()).next.clone()
        }
    }

    #[inline]
    pub(super) fn set_next(&self, new_next: Option<PrintItemPath<TString, TInfo, TCondition>>) {
        unsafe {
            (*self.value.get()).set_next(new_next);
        }
    }

    #[inline]
    pub(super) fn get_last_next(&self) -> Option<PrintItemPath<TString, TInfo, TCondition>> {
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

        return current;
    }

    /// Gets the node unsafely. Be careful when using this and ensure no mutation is
    /// happening during the borrow.
    #[inline]
    pub(super) unsafe fn get_node(&self) -> *mut PrintNode<TString, TInfo, TCondition> {
        self.value.get()
    }

    #[inline]
    pub fn take_next(self) -> Option<PrintItemPath<TString, TInfo, TCondition>> {
        self.value.into_inner().next.take()
    }
}

pub type PrintItemPath<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> = Rc<PrintNodeCell<TString, TInfo, TCondition>>;

/* Print item and kinds */

/// The different items the printer could encounter.
pub enum PrintItem<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    String(Rc<StringContainer<TString>>),
    Condition(Rc<TCondition>),
    Info(Rc<TInfo>),
    Signal(Signal),
    RcPath(PrintItemPath<TString, TInfo, TCondition>),
}

impl<TString, TInfo, TCondition> Clone for PrintItem<TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    fn clone(&self) -> PrintItem<TString, TInfo, TCondition> {
        match self {
            PrintItem::String(text) => PrintItem::String(text.clone()),
            PrintItem::Condition(condition) => PrintItem::Condition(condition.clone()),
            PrintItem::Info(info) => PrintItem::Info(info.clone()),
            PrintItem::Signal(signal) => PrintItem::Signal(*signal),
            PrintItem::RcPath(path) => PrintItem::RcPath(path.clone()),
        }
    }
}

#[derive(Clone, PartialEq, Copy, Debug)]
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
}

/// Can be used to get information at a certain location being printed. These
/// can be resolved by providing the info object to a condition context's
/// get_resolved_info(&info) method.
#[derive(Clone, PartialEq, Copy, Debug)]
pub struct Info {
    /// Unique identifier.
    id: usize,
    /// Name for debugging purposes.
    name: &'static str,
}

impl InfoTrait for Info {
    fn get_unique_id(&self) -> usize {
        self.id
    }

    fn get_name(&self) -> &'static str {
        self.name
    }
}

static INFO_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl Info {
    pub fn new(name: &'static str) -> Info {
        Info {
            id: INFO_COUNTER.fetch_add(1, Ordering::SeqCst),
            name
        }
    }
}

/// Conditionally print items based on a condition.
///
/// These conditions are extremely flexible and can even be resolved based on
/// information found later on in the file.
pub struct Condition<TString = String, TInfo = Info> where TString : StringTrait, TInfo : InfoTrait {
    /// Unique identifier.
    id: usize,
    /// Name for debugging purposes.
    name: &'static str,
    /// If a reference has been created for the condition via `get_reference()`. If so, the printer
    /// will store the condition and it will be retrievable via a condition resolver.
    is_stored: bool,
    /// The condition to resolve.
    pub condition: Rc<Box<ConditionResolver<TString, TInfo, Condition<TString, TInfo>>>>,
    /// The items to print when the condition is true.
    pub true_path: Option<PrintItemPath<TString, TInfo, Condition<TString, TInfo>>>,
    /// The items to print when the condition is false or undefined (not yet resolved).
    pub false_path: Option<PrintItemPath<TString, TInfo, Condition<TString, TInfo>>>,
    /// Any infos that should cause the re-evaluation of this condition.
    /// This is only done on request for performance reasons.
    pub(super) dependent_infos: Option<Vec<TInfo>>,
}

impl Clone for Condition {
    fn clone(&self) -> Condition {
        return Condition {
            id: self.id,
            is_stored: self.is_stored,
            name: self.name,
            condition: self.condition.clone(),
            true_path: self.true_path.clone(),
            false_path: self.false_path.clone(),
            dependent_infos: self.dependent_infos.clone(),
        };
    }
}

static CONDITION_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl<TString, TInfo> Condition<TString, TInfo> where TString : StringTrait, TInfo : InfoTrait {
    pub fn new(name: &'static str, properties: ConditionProperties<TString, TInfo>) -> Condition<TString, TInfo> {
        Condition::new_internal(name, properties, None)
    }

    pub fn new_true() -> Condition<TString, TInfo> {
        Condition::new_internal("trueCondition", ConditionProperties {
            condition: Box::new(|_| Some(true)),
            true_path: None,
            false_path: None,
        }, None)
    }

    pub fn new_false() -> Condition<TString, TInfo> {
        Condition::new_internal("falseCondition", ConditionProperties {
            condition: Box::new(|_| Some(false)),
            true_path: None,
            false_path: None,
        }, None)
    }

    pub fn new_with_dependent_infos(name: &'static str, properties: ConditionProperties<TString, TInfo>, dependent_infos: Vec<TInfo>) -> Condition<TString, TInfo> {
        Condition::new_internal(name, properties, Some(dependent_infos))
    }

    fn new_internal(name: &'static str, properties: ConditionProperties<TString, TInfo>, dependent_infos: Option<Vec<TInfo>>) -> Condition<TString, TInfo> {
        Condition {
            id: CONDITION_COUNTER.fetch_add(1, Ordering::SeqCst),
            is_stored: dependent_infos.is_some(),
            name,
            condition: Rc::new(properties.condition),
            true_path: properties.true_path.map(|x| x.first_node).flatten(),
            false_path: properties.false_path.map(|x| x.first_node).flatten(),
            dependent_infos,
        }
    }

    pub fn get_reference(&mut self) -> ConditionReference {
        self.is_stored = true;
        ConditionReference::new(self.name, self.id)
    }
}

#[derive(Clone, PartialEq, Copy, Debug)]
pub struct ConditionReference {
    pub(super) name: &'static str,
    pub(super) id: usize,
}

impl ConditionReference {
    pub fn new(name: &'static str, id: usize) -> ConditionReference {
        ConditionReference { name, id }
    }

    /// Creates a condition resolver that checks the value of the condition this references.
    pub fn create_resolver(&self) -> impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static {
        let captured_self = self.clone();
        move |condition_context: &mut ConditionResolverContext| {
            condition_context.get_resolved_condition(&captured_self)
        }
    }
}

impl<TString, TInfo> ConditionTrait<TString, TInfo, Condition<TString, TInfo>> for Condition<TString, TInfo> where TString : StringTrait, TInfo : InfoTrait {
    #[inline]
    fn get_unique_id(&self) -> usize {
        self.id
    }

    #[inline]
    fn get_name(&self) -> &'static str {
        self.name
    }

    #[inline]
    fn get_is_stored(&self) -> bool {
        self.is_stored
    }

    #[inline]
    fn resolve(&self, context: &mut ConditionResolverContext<TString, TInfo, Self>) -> Option<bool> {
        (self.condition)(context)
    }

    #[inline]
    fn get_true_path(&self) -> Option<PrintItemPath<TString, TInfo, Condition<TString, TInfo>>> {
        self.true_path.clone()
    }

    #[inline]
    fn get_false_path(&self) -> Option<PrintItemPath<TString, TInfo, Condition<TString, TInfo>>> {
        self.false_path.clone()
    }

    #[inline]
    fn get_dependent_infos<'a>(&'a self) -> &'a Option<Vec<TInfo>> {
        &self.dependent_infos
    }
}

/// Properties for the condition.
pub struct ConditionProperties<TString = String, TInfo = Info> where TString : StringTrait, TInfo : InfoTrait {
    /// The condition to resolve.
    pub condition: Box<ConditionResolver<TString, TInfo, Condition<TString, TInfo>>>,
    /// The items to print when the condition is true.
    pub true_path: Option<PrintItems<TString, TInfo, Condition<TString, TInfo>>>,
    /// The items to print when the condition is false or undefined (not yet resolved).
    pub false_path: Option<PrintItems<TString, TInfo, Condition<TString, TInfo>>>,
}

/// Function used to resolve a condition.
pub type ConditionResolver<TString = String, TInfo = Info, TCondition = Condition> = dyn Fn(&mut ConditionResolverContext<TString, TInfo, TCondition>) -> Option<bool>; // todo: impl Fn(etc) -> etc; once supported

/// Context used when resolving a condition.
pub struct ConditionResolverContext<'a, TString = String, TInfo = Info, TCondition = Condition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    printer: &'a mut Printer<TString, TInfo, TCondition>,
    /// Gets the writer info at the condition's location.
    pub writer_info: WriterInfo,
}

impl<'a, TString, TInfo, TCondition> ConditionResolverContext<'a, TString, TInfo, TCondition> where TString : StringTrait, TInfo : InfoTrait, TCondition : ConditionTrait<TString, TInfo, TCondition> {
    pub fn new(printer: &'a mut Printer<TString, TInfo, TCondition>) -> Self {
        let writer_info = printer.get_writer_info();
        ConditionResolverContext {
            printer,
            writer_info,
        }
    }

    /// Gets if a condition was true, false, or returns undefined when not yet resolved.
    /// A condition reference can be retrieved by calling the `get_reference()` on a condition.
    pub fn get_resolved_condition(&mut self, condition_reference: &ConditionReference) -> Option<bool> {
        self.printer.get_resolved_condition(condition_reference)
    }

    /// Gets the writer info at a specified info or returns undefined when not yet resolved.
    pub fn get_resolved_info(&self, info: &TInfo) -> Option<&WriterInfo> {
        self.printer.get_resolved_info(info)
    }
}

/// A container that holds the string's value and character count.
#[derive(Clone)]
pub struct StringContainer<TString> where TString : StringTrait {
    /// The string value.
    pub text: TString,
    /// The cached character count.
    /// It is much faster to cache this than to recompute it all the time.
    pub(super) char_count: u32,
}

impl<TString> StringContainer<TString> where TString : StringTrait {
    /// Creates a new string container.
    pub fn new(text: TString) -> StringContainer<TString> {
        let char_count = text.get_length() as u32;
        StringContainer {
            text,
            char_count
        }
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
}

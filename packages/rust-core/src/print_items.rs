use super::printer::Printer;
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::mem;

// Traits. This allows implementing these for Wasm objects.

pub trait StringRef {
    fn get_length(&self) -> usize;
    fn get_text<'a>(&'a self) -> &'a str;
    fn get_text_clone(&self) -> String;
}

impl StringRef for String {
    fn get_length(&self) -> usize {
        self.chars().count()
    }

    fn get_text<'a>(&'a self) -> &'a str {
        self
    }

    fn get_text_clone(&self) -> String {
        self.clone()
    }
}

pub trait InfoRef {
    fn get_unique_id(&self) -> usize;
    fn get_name(&self) -> &'static str;
}

pub trait ConditionRef<TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    fn get_unique_id(&self) -> usize;
    fn get_name(&self) -> &'static str;
    fn resolve(&self, context: &mut ConditionResolverContext<TString, TInfo, TCondition>) -> Option<bool>;
    fn get_true_path(&self) -> Option<Rc<RefCell<ConditionPath<TString, TInfo, TCondition>>>>;
    fn get_false_path(&self) -> Option<Rc<RefCell<ConditionPath<TString, TInfo, TCondition>>>>;
}

pub struct PrintItems<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    pub(super) first_node: Option<Rc<RefCell<PrintNode<TString, TInfo, TCondition>>>>,
    last_node: Option<Rc<RefCell<PrintNode<TString, TInfo, TCondition>>>>,
}

pub struct ConditionPath<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    pub(super) first_node: Option<Rc<RefCell<PrintNode<TString, TInfo, TCondition>>>>,
    last_node: Option<Rc<RefCell<PrintNode<TString, TInfo, TCondition>>>>,
    has_next_set: bool,
}

impl<TString, TInfo, TCondition> ConditionPath<TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    pub(super) fn set_next_node_if_not_set(&mut self, next_node: Option<Rc<RefCell<PrintNode<TString, TInfo, TCondition>>>>) {
        if self.has_next_set { return; }

        if let Some(last_node) = self.last_node.as_ref().or(self.first_node.as_ref()) {
            last_node.borrow_mut().set_next(next_node);
        }

        self.has_next_set = true;
    }
}


impl<TString, TInfo, TCondition> PrintItems<TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    pub fn new() -> PrintItems<TString, TInfo, TCondition> {
        PrintItems {
            first_node: None,
            last_node: None,
        }
    }
}

impl PrintItems {
    pub fn push(&mut self, node: PrintNode) {
        let node = Rc::new(RefCell::new(node));
        if let Some(first_node) = &self.first_node {
            let new_last_node = node.get_last_next_or_self();
            self.last_node.as_ref().unwrap_or(first_node).borrow_mut().set_next(Some(node));
            self.last_node = Some(new_last_node);
        } else {
            self.first_node = Some(node);
        }
    }

    pub fn extend(&mut self, items: PrintItems) {
        if let Some(first_node) = &self.first_node {
            self.last_node.as_ref().unwrap_or(first_node).borrow_mut().set_next(items.first_node.clone());
            self.last_node = items.last_node.or(items.first_node.or(self.last_node.clone())); // todo: fix this
        } else {
            self.first_node = items.first_node;
            self.last_node = items.last_node;
        }
    }

    pub fn push_str(&mut self, item: &str) {
        self.push(PrintNode::new(PrintItem::String(Rc::from(String::from(item)))));
    }

    pub fn push_condition(&mut self, condition: Condition) {
        self.push(PrintNode::new(PrintItem::Condition(condition)));
    }

    pub fn push_info(&mut self, info: Info) {
        self.push(PrintNode::new(PrintItem::Info(Rc::from(info))));
    }

    pub fn push_signal(&mut self, signal: Signal) {
        self.push(PrintNode::new(PrintItem::Signal(Rc::new(signal))));
    }
}

impl Clone for PrintItems {
    fn clone(&self) -> PrintItems {
        let mut items = PrintItems::new();
        let mut next = self.first_node.clone();
        while let Some(current) = next {
            items.push(PrintNode {
                item: current.borrow().item.clone(),
                next: None,
            });
            next = current.borrow().next.clone();
        }

        items
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

pub struct PrintNode<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    pub(super) next: Option<Rc<RefCell<PrintNode<TString, TInfo, TCondition>>>>,
    pub(super) item: PrintItem<TString, TInfo, TCondition>,
}

impl<TString, TInfo, TCondition> PrintNode<TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    fn new(item: PrintItem<TString, TInfo, TCondition>) -> PrintNode<TString, TInfo, TCondition> {
        PrintNode {
            item,
            next: None,
        }
    }

    fn set_next(&mut self, new_next: Option<Rc<RefCell<PrintNode<TString, TInfo, TCondition>>>>) {
        let past_next = mem::replace(&mut self.next, new_next.clone());

        if let Some(past_next) = past_next {
            if let Some(new_next) = new_next {
                new_next.get_last_next_or_self().borrow_mut().set_next(Some(past_next));
            }
        }
    }
}

trait RcPrintNodeGetLast<TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    fn get_last_next_or_self(&self) -> Rc<RefCell<PrintNode<TString, TInfo, TCondition>>>;
}

impl<TString, TInfo, TCondition> RcPrintNodeGetLast<TString, TInfo, TCondition> for Rc<RefCell<PrintNode<TString, TInfo, TCondition>>> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    fn get_last_next_or_self(&self) -> Rc<RefCell<PrintNode<TString, TInfo, TCondition>>> {
        let mut last = self.clone();
        while let Some(next) = last.clone().borrow().next.clone() {
            last = next;
        }
        return last;
    }
}

/// The different items the printer could encounter.
#[derive(Clone)]
pub enum PrintItem<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    String(Rc<TString>),
    Condition(TCondition), // no Rc because conditions shouldn't be shared since paths must be unique
    Info(Rc<TInfo>),
    Signal(Rc<Signal>),
}

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
}

/// Can be used to get information at a certain location being printed. These
/// can be resolved by providing the info object to a condition context's
/// get_resolved_info(&info) method.
#[derive(Clone)]
pub struct Info {
    /// Unique identifier.
    id: usize,
    /// Name for debugging purposes.
    pub name: &'static str,
}

impl InfoRef for Info {
    fn get_unique_id(&self) -> usize {
        self.id
    }

    fn get_name(&self) -> &'static str {
        self.name
    }
}

impl<TString, TCondition> Into<PrintItem<TString, Info, TCondition>> for Info where TString : StringRef, TCondition : ConditionRef<TString, Info, TCondition> {
    fn into(self) -> PrintItem<TString, Info, TCondition> {
        PrintItem::Info(Rc::new(self))
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
pub struct Condition<TString = String, TInfo = Info> where TString : StringRef, TInfo : InfoRef {
    /// Unique identifier.
    id: usize,
    /// Name for debugging purposes.
    name: &'static str,
    /// The condition to resolve.
    pub condition: Rc<Box<ConditionResolver<TString, TInfo, Condition<TString, TInfo>>>>,
    /// The items to print when the condition is true.
    pub true_path: Option<Rc<RefCell<ConditionPath<TString, TInfo, Condition<TString, TInfo>>>>>,
    /// The items to print when the condition is false or undefined (not yet resolved).
    pub false_path: Option<Rc<RefCell<ConditionPath<TString, TInfo, Condition<TString, TInfo>>>>>,
}

// need to manually implement this for some reason instead of using #[derive(Clone)]
impl<TString, TInfo> Clone for Condition<TString, TInfo> where TString : StringRef, TInfo : InfoRef {
    fn clone(&self) -> Condition<TString, TInfo> {
        Condition {
            id: self.id,
            name: self.name,
            condition: self.condition.clone(),
            true_path: self.true_path.clone(),
            false_path: self.false_path.clone(),
        }
    }
}

static CONDITION_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl<TString, TInfo> Condition<TString, TInfo> where TString : StringRef, TInfo : InfoRef {
    pub fn new(name: &'static str, properties: ConditionProperties<TString, TInfo>) -> Condition<TString, TInfo> {
        Condition {
            id: CONDITION_COUNTER.fetch_add(1, Ordering::SeqCst),
            name,
            condition: Rc::new(properties.condition),
            true_path: properties.true_path.map(|x| Rc::new(RefCell::new(ConditionPath {
                first_node: x.first_node,
                last_node: x.last_node,
                has_next_set: false,
            }))),
            false_path: properties.false_path.map(|x| Rc::new(RefCell::new(ConditionPath {
                first_node: x.first_node,
                last_node: x.last_node,
                has_next_set: false,
            }))),
        }
    }
}

impl<TString, TInfo> ConditionRef<TString, TInfo, Condition<TString, TInfo>> for Condition<TString, TInfo> where TString : StringRef, TInfo : InfoRef {
    fn get_unique_id(&self) -> usize {
        self.id
    }

    fn get_name(&self) -> &'static str {
        self.name
    }

    fn resolve(&self, context: &mut ConditionResolverContext<TString, TInfo, Self>) -> Option<bool> {
        (self.condition)(context)
    }

    fn get_true_path(&self) -> Option<Rc<RefCell<ConditionPath<TString, TInfo, Condition<TString, TInfo>>>>> {
        self.true_path.clone()
    }

    fn get_false_path(&self) -> Option<Rc<RefCell<ConditionPath<TString, TInfo, Condition<TString, TInfo>>>>> {
        self.false_path.clone()
    }
}

impl<TString, TInfo> Into<PrintItem<TString, TInfo, Condition<TString, TInfo>>> for Condition<TString, TInfo> where TString : StringRef, TInfo : InfoRef {
    fn into(self) -> PrintItem<TString, TInfo, Condition<TString, TInfo>> {
        PrintItem::Condition(self)
    }
}

/// Properties for the condition.
pub struct ConditionProperties<TString = String, TInfo = Info> where TString : StringRef, TInfo : InfoRef {
    /// The condition to resolve.
    pub condition: Box<ConditionResolver<TString, TInfo, Condition<TString, TInfo>>>,
    /// The items to print when the condition is true.
    pub true_path: Option<PrintItems<TString, TInfo, Condition<TString, TInfo>>>,
    /// The items to print when the condition is false or undefined (not yet resolved).
    pub false_path: Option<PrintItems<TString, TInfo, Condition<TString, TInfo>>>,
}

/// Function used to resolve a condition.
pub type ConditionResolver<TString = String, TInfo = Info, TCondition = Condition> = dyn Fn(&mut ConditionResolverContext<TString, TInfo, TCondition>) -> Option<bool>; // todo: impl Fn(etc) -> etc + Clone + 'static; once supported

/// Context used when resolving a condition.
pub struct ConditionResolverContext<'a, TString = String, TInfo = Info, TCondition = Condition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    printer: &'a mut Printer<TString, TInfo, TCondition>,
    /// Gets the writer info at the condition's location.
    pub writer_info: WriterInfo,
}

impl<'a, TString, TInfo, TCondition> ConditionResolverContext<'a, TString, TInfo, TCondition> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    pub fn new(printer: &'a mut Printer<TString, TInfo, TCondition>) -> Self {
        let writer_info = printer.get_writer_info();
        ConditionResolverContext {
            printer,
            writer_info,
        }
    }

    /// Gets if a condition was true, false, or returns undefined when not yet resolved.
    pub fn get_resolved_condition(&mut self, condition: &TCondition) -> Option<bool> {
        self.printer.get_resolved_condition(condition)
    }

    /// Gets the writer info at a specified info or returns undefined when not yet resolved.
    pub fn get_resolved_info(&mut self, info: &TInfo) -> Option<WriterInfo> {
        self.printer.get_resolved_info(info)
    }
}

/// Information about a certain location being printed.
#[derive(Clone)]
pub struct WriterInfo {
    pub line_number: u32,
    pub column_number: u32,
    pub indent_level: u16,
    pub line_start_indent_level: u16,
    pub line_start_column_number: u32,
}

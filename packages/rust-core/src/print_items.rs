use super::printer::Printer;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

// Traits. This allows implementing these for Wasm objects.

pub trait StringRef : Clone {
    fn get_length(&self) -> usize;
    fn get_text(self) -> String;
}

impl StringRef for String {
    fn get_length(&self) -> usize {
        self.chars().count()
    }

    fn get_text(self) -> String {
        self
    }
}

pub trait InfoRef : Clone {
    fn get_unique_id(&self) -> usize;
    fn get_name(&self) -> &'static str;
}

pub trait ConditionRef<TString, TInfo, TCondition> : Clone where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    fn get_unique_id(&self) -> usize;
    fn get_name(&self) -> &'static str;
    fn resolve(&self, context: &mut ConditionResolverContext<TString, TInfo, TCondition>) -> Option<bool>;
    fn get_true_path(&self) -> Option<Rc<Vec<PrintItem<TString, TInfo, TCondition>>>>;
    fn get_false_path(&self) -> Option<Rc<Vec<PrintItem<TString, TInfo, TCondition>>>>;
}

/// The different items the printer could encounter.
#[derive(Clone)]
pub enum PrintItem<TString = String, TInfo = Info, TCondition = Condition<TString, TInfo>> where TString : StringRef, TInfo : InfoRef, TCondition : ConditionRef<TString, TInfo, TCondition> {
    String(TString),
    Condition(TCondition),
    Info(TInfo),
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

impl<TInfo, TCondition> Into<PrintItem<String, TInfo, TCondition>> for &str where TInfo : InfoRef, TCondition : ConditionRef<String, TInfo, TCondition> {
    fn into(self) -> PrintItem<String, TInfo, TCondition> {
        PrintItem::String(String::from(self))
    }
}

impl<TInfo, TCondition> Into<PrintItem<String, TInfo, TCondition>> for String where TInfo : InfoRef, TCondition : ConditionRef<String, TInfo, TCondition> {
    fn into(self) -> PrintItem<String, TInfo, TCondition> {
        PrintItem::String(self)
    }
}

impl<TInfo, TCondition> Into<PrintItem<String, TInfo, TCondition>> for &String where TInfo : InfoRef, TCondition : ConditionRef<String, TInfo, TCondition> {
    fn into(self) -> PrintItem<String, TInfo, TCondition> {
        PrintItem::String(self.clone())
    }
}

/// Can be used to get information at a certain location being printed. These
/// can be resolved by providing the info object to a condition context's
/// getResolvedInfo method.
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
        PrintItem::Info(self)
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

    pub fn into_clone<TString, TCondition>(&self) -> PrintItem<TString, Info, TCondition> where TString : StringRef, TCondition : ConditionRef<TString, Info, TCondition> {
        PrintItem::Info(self.clone())
    }
}

/// Conditionally print items based on a condition.
///
/// These conditions are extremely flexible and can even be resolved based on
/// information found later on in the file.
#[derive(Clone)]
pub struct Condition<TString = String, TInfo = Info> where TString : StringRef, TInfo : InfoRef {
    /// Unique identifier.
    id: usize,
    /// Name for debugging purposes.
    name: &'static str,
    /// The condition to resolve.
    pub condition: Rc<Box<ConditionResolver<TString, TInfo, Condition<TString, TInfo>>>>,
    /// The items to print when the condition is true.
    pub true_path: Option<Rc<Vec<PrintItem<TString, TInfo, Condition<TString, TInfo>>>>>,
    /// The items to print when the condition is false or undefined (not yet resolved).
    pub false_path: Option<Rc<Vec<PrintItem<TString, TInfo, Condition<TString, TInfo>>>>>,
}

static CONDITION_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl<TString, TInfo> Condition<TString, TInfo> where TString : StringRef, TInfo : InfoRef {
    pub fn new(name: &'static str, properties: ConditionProperties<TString, TInfo>) -> Condition<TString, TInfo> {
        Condition {
            id: CONDITION_COUNTER.fetch_add(1, Ordering::SeqCst),
            name,
            condition: Rc::new(properties.condition),
            true_path: properties.true_path.map(|x| Rc::new(x)),
            false_path: properties.false_path.map(|x| Rc::new(x)),
        }
    }

    pub fn into_clone(&self) -> PrintItem<TString, TInfo, Condition<TString, TInfo>> {
        PrintItem::Condition(self.clone())
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

    fn get_true_path(&self) -> Option<Rc<Vec<PrintItem<TString, TInfo, Self>>>> {
        self.true_path.clone()
    }

    fn get_false_path(&self) -> Option<Rc<Vec<PrintItem<TString, TInfo, Self>>>> {
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
    pub true_path: Option<Vec<PrintItem<TString, TInfo, Condition<TString, TInfo>>>>,
    /// The items to print when the condition is false or undefined (not yet resolved).
    pub false_path: Option<Vec<PrintItem<TString, TInfo, Condition<TString, TInfo>>>>,
}

/// Function used to resolve a condition.
pub type ConditionResolver<TString, TInfo, TCondition> = dyn Fn(&mut ConditionResolverContext<TString, TInfo, TCondition>) -> Option<bool>; // todo: impl Fn(etc) -> etc + Clone + 'static; once supported

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

    /// Gets if a condition was true, false, or returns the provded default value when not yet resolved.
    pub fn get_resolved_condition_or_default(&mut self, condition: &TCondition, default_value: bool) -> bool {
        match self.get_resolved_condition(condition) {
            Some(x) => x,
            _ => default_value,
        }
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

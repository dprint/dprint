use super::StringContainer;
use super::printer::Printer;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The different items the printer could encounter.
#[derive(Clone)]
pub enum PrintItem<T = String> where T : StringContainer {
    String(T),
    Condition(Condition<T>),
    Info(Info),
    /// Signal that a tab should occur based on the printer settings.
    Tab,
    /// Signal that a new line should occur based on the printer settings.
    NewLine,
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

impl Into<PrintItem<String>> for &str {
    fn into(self) -> PrintItem<String> {
        PrintItem::String(String::from(self))
    }
}

impl Into<PrintItem<String>> for String {
    fn into(self) -> PrintItem<String> {
        PrintItem::String(self)
    }
}

impl Into<PrintItem<String>> for &String {
    fn into(self) -> PrintItem<String> {
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

impl<T> Into<PrintItem<T>> for Info where T : StringContainer {
    fn into(self) -> PrintItem<T> {
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

    pub fn get_unique_id(&self) -> usize {
        self.id
    }

    pub fn into_clone<T>(&self) -> PrintItem<T> where T : StringContainer {
        PrintItem::Info(self.clone())
    }
}

/// Conditionally print items based on a condition.
///
/// These conditions are extremely flexible and can even be resolved based on
/// information found later on in the file.
#[derive(Clone)]
pub struct Condition<T = String> where T : StringContainer {
    /// Unique identifier.
    id: usize,
    /// Name for debugging purposes.
    pub name: &'static str,
    /// The condition to resolve.
    pub condition: Rc<Box<ConditionResolver<T>>>,
    /// The items to print when the condition is true.
    pub true_path: Option<Rc<Vec<PrintItem<T>>>>,
    /// The items to print when the condition is false or undefined (not yet resolved).
    pub false_path: Option<Rc<Vec<PrintItem<T>>>>,
}

static CONDITION_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl<T> Condition<T> where T : StringContainer {
    pub fn new(name: &'static str, properties: ConditionProperties<T>) -> Condition<T> {
        Condition {
            id: CONDITION_COUNTER.fetch_add(1, Ordering::SeqCst),
            name,
            condition: Rc::new(properties.condition),
            true_path: properties.true_path.map(|x| Rc::new(x)),
            false_path: properties.false_path.map(|x| Rc::new(x)),
        }
    }

    pub fn get_unique_id(&self) -> usize {
        self.id
    }

    pub fn into_clone(&self) -> PrintItem<T> {
        PrintItem::Condition(self.clone())
    }
}

impl<T> Into<PrintItem<T>> for Condition<T> where T : StringContainer {
    fn into(self) -> PrintItem<T> {
        PrintItem::Condition(self)
    }
}

/// Properties for the condition.
pub struct ConditionProperties<T = String> where T : StringContainer {
    /// The condition to resolve.
    pub condition: Box<ConditionResolver<T>>,
    /// The items to print when the condition is true.
    pub true_path: Option<Vec<PrintItem<T>>>,
    /// The items to print when the condition is false or undefined (not yet resolved).
    pub false_path: Option<Vec<PrintItem<T>>>,
}

/// Function used to resolve a condition.
pub type ConditionResolver<T> = dyn Fn(&mut ResolveConditionContext<T>) -> Option<bool>; // todo: impl Fn(etc) -> etc + Clone + 'static; once supported

/// Context used when resolving a condition.
pub struct ResolveConditionContext<'a, T = String> where T : StringContainer {
    printer: &'a mut Printer<T>,
    /// Gets the writer info at the condition's location.
    pub writer_info: WriterInfo,
}

impl<'a, T> ResolveConditionContext<'a, T> where T : StringContainer {
    pub fn new(printer: &'a mut Printer<T>) -> Self {
        let writer_info = printer.get_writer_info();
        ResolveConditionContext {
            printer,
            writer_info,
        }
    }

    /// Gets if a condition was true, false, or returns undefined when not yet resolved.
    pub fn get_resolved_condition(&mut self, condition: &Condition<T>) -> Option<bool> {
        self.printer.get_resolved_condition(condition)
    }

    /// Gets if a condition was true, false, or returns the provded default value when not yet resolved.
    pub fn get_resolved_condition_or_default(&mut self, condition: &Condition<T>, default_value: bool) -> bool {
        match self.get_resolved_condition(condition) {
            Some(x) => x,
            _ => default_value,
        }
    }

    /// Gets the writer info at a specified info or returns undefined when not yet resolved.
    pub fn get_resolved_info(&mut self, info: &Info) -> Option<WriterInfo> {
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

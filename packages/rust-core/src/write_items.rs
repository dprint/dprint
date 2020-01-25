use std::rc::Rc;
use super::StringContainer;
use super::StringRef;

pub enum WriteItem<T = String> where T : StringRef {
    String(Rc<StringContainer<T>>),
    Indent(u8),
    NewLine,
    Tab,
    Space,
}

// for some reason #[derive(Clone)] was not working, so manually implement this...
impl<TString> Clone for WriteItem<TString> where TString : StringRef {
    fn clone(&self) -> WriteItem<TString> {
        match self {
            WriteItem::Indent(times) => WriteItem::Indent(*times),
            WriteItem::NewLine => WriteItem::NewLine,
            WriteItem::Tab => WriteItem::Tab,
            WriteItem::Space => WriteItem::Space,
            WriteItem::String(text) => WriteItem::String(text.clone()),
        }
    }
}
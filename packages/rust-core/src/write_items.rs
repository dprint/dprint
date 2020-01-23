use super::StringRef;

pub enum WriteItem<'a, T = String> where T : StringRef {
    String(&'a T),
    Indent,
    NewLine,
    Tab,
    Space,
}

// for some reason #[derive(Clone)] was not working, so manually implement this...
impl<'a, TString> Clone for WriteItem<'a, TString> where TString : StringRef {
    fn clone(&self) -> WriteItem<'a, TString> {
        match self {
            WriteItem::Indent => WriteItem::Indent,
            WriteItem::NewLine => WriteItem::NewLine,
            WriteItem::Tab => WriteItem::Tab,
            WriteItem::Space => WriteItem::Space,
            WriteItem::String(text) => WriteItem::String(text),
        }
    }
}
use super::StringRef;

#[derive(Clone)]
pub enum WriteItem<T = String> where T : StringRef {
    String(T),
    Indent,
    NewLine,
    Tab,
    Space,
}

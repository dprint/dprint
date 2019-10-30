use super::StringContainer;

#[derive(Clone)]
pub enum WriteItem<T = String> where T : StringContainer {
    String(T),
    Indent,
    NewLine,
    Tab,
    Space,
}

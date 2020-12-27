use super::StringContainer;

#[derive(Clone)]
pub enum WriteItem<'a> {
    String(&'a StringContainer),
    Indent(u8),
    NewLine,
    Tab,
    Space,
}

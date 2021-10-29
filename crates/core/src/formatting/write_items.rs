use super::StringContainer;

#[derive(Clone, Copy)]
pub enum WriteItem<'a> {
  String(&'a StringContainer),
  Indent(u8),
  NewLine,
  Tab,
  Space,
}

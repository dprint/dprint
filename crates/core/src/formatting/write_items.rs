use std::rc::Rc;
use super::StringContainer;

#[derive(Clone)]
pub enum WriteItem {
    String(Rc<StringContainer>),
    Indent(u8),
    NewLine,
    Tab,
    Space,
}

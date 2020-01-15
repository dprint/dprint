use super::*;

/// Converts the write items into JS objects for use in JavaScript.
pub fn get_js_write_items<T>(write_items: T) -> Vec<JsValue> where T : Iterator<Item = WriteItem<JsString>> {
    write_items.into_iter().map(|item| -> JsValue {
        match item {
            WriteItem::String(text) => Rc::try_unwrap(text).ok().unwrap().reference.into(),
            WriteItem::Indent => js_sys::Number::from(0).into(),
            WriteItem::NewLine => js_sys::Number::from(1).into(),
            WriteItem::Tab => js_sys::Number::from(2).into(),
            WriteItem::Space => js_sys::Number::from(3).into(),
        }
    }).collect()
}

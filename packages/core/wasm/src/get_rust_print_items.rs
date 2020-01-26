use wasm_bindgen::*;
use super::*;

pub fn get_rust_print_items(print_items: Vec<JsValue>) -> PrintItems<JsString, JsInfo, JsCondition> {
    let mut items = PrintItems::new();
    for item in print_items.into_iter() {
        if item.is_string() {
            let js_str = JsString::new(item.dyn_into::<js_sys::JsString>().ok().unwrap());
            items.push_item(PrintItem::String(Rc::new(StringContainer::new(js_str))));
        } else if let Some(value) = item.dyn_ref::<js_sys::Number>() {
            let num: u8 = value.value_of() as u8;
            items.push_item(PrintItem::Signal(match num {
                0 => Signal::NewLine,
                1 => Signal::Tab,
                2 => Signal::PossibleNewLine,
                3 => Signal::SpaceOrNewLine,
                4 => Signal::ExpectNewLine,
                5 => Signal::StartIndent,
                6 => Signal::FinishIndent,
                7 => Signal::StartNewLineGroup,
                8 => Signal::FinishNewLineGroup,
                9 => Signal::SingleIndent,
                10 => Signal::StartIgnoringIndent,
                11 => Signal::FinishIgnoringIndent,
                _ => panic!("Not implemented value: {}", num),
            }))
        } else {
            let value = item.unchecked_into::<JsConditionOrInfo>();
            let kind = value.kind();
            if kind == 0 {
                items.push_item(PrintItem::Condition(Rc::new(JsCondition::new(value.unchecked_into::<RawJsCondition>()))))
            } else if kind == 1 {
                items.push_item(PrintItem::Info(Rc::new(value.unchecked_into::<JsInfo>())))
            } else {
                panic!("Unknown print item kind: {}", kind);
            }
        }
    }
    items
}

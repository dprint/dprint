use wasm_bindgen::*;
use super::*;

pub fn get_rust_print_items(print_items: Vec<JsValue>) -> Vec<PrintItem<JsString, JsInfo, JsCondition>> {
    print_items.into_iter().map(|item| -> PrintItem<JsString, JsInfo, JsCondition> {
        if item.is_string() {
            PrintItem::String(Rc::new(JsString {
                reference: item.dyn_into::<js_sys::JsString>().ok().unwrap()
            }))
        } else if let Some(value) = item.dyn_ref::<js_sys::Number>() {
            let num: u8 = value.value_of() as u8;
            match num {
                0 => PrintItem::NewLine,
                1 => PrintItem::Tab,
                2 => PrintItem::PossibleNewLine,
                3 => PrintItem::SpaceOrNewLine,
                4 => PrintItem::ExpectNewLine,
                5 => PrintItem::StartIndent,
                6 => PrintItem::FinishIndent,
                7 => PrintItem::StartNewLineGroup,
                8 => PrintItem::FinishNewLineGroup,
                9 => PrintItem::SingleIndent,
                10 => PrintItem::StartIgnoringIndent,
                11 => PrintItem::FinishIgnoringIndent,
                _ => panic!("Not implemented value: {}", num),
            }
        } else {
            let value = item.unchecked_into::<JsConditionOrInfo>();
            let kind = value.kind();
            if kind == 0 {
                PrintItem::Condition(Rc::new(value.unchecked_into::<JsCondition>()))
            } else if kind == 1 {
                PrintItem::Info(Rc::new(value.unchecked_into::<JsInfo>()))
            } else {
                panic!("Unknown print item kind: {}", kind);
            }
        }
    }).collect()
}

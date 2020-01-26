use wasm_bindgen::prelude::*;
use super::*;

#[wasm_bindgen]
pub fn get_write_items(print_items: Vec<JsValue>, max_width: u32, indent_width: u8, is_testing: bool) -> Vec<JsValue> {
    console_error_panic_hook::set_once();

    let rust_print_items = get_rust_print_items(print_items);
    let write_items = dprint_core::get_write_items(&rust_print_items, dprint_core::GetWriteItemsOptions {
        indent_width: indent_width,
        is_testing: is_testing,
        max_width: max_width
    });
    get_js_write_items(write_items)
}

use wasm_bindgen::prelude::*;
use wasm_bindgen::*;
use dprint_core::*;
use super::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = JsConditionOrInfo)]
    pub type JsConditionOrInfo;

    #[wasm_bindgen(method, getter)]
    pub fn kind(this: &JsConditionOrInfo) -> u8;

    // JsInfo
    #[wasm_bindgen(js_name = JsInfo)]
    pub type JsInfo;

    #[wasm_bindgen(method, getter)]
    pub fn name(this: &JsInfo) -> js_sys::JsString;

    #[wasm_bindgen(method, getter)]
    pub fn id(this: &JsInfo) -> usize;

    // JsCondition
    #[wasm_bindgen(js_name = JsCondition)]
    pub type JsCondition;

    #[wasm_bindgen(method, getter)]
    pub fn name(this: &JsCondition) -> js_sys::JsString;

    #[wasm_bindgen(method, getter)]
    pub fn id(this: &JsCondition) -> usize;

    #[wasm_bindgen(method)]
    pub fn condition(this: &JsCondition, context: JsConditionResolverContext) -> Option<bool>;

    #[wasm_bindgen(method, getter, js_name = truePath)]
    pub fn true_path(this: &JsCondition) -> Option<Vec<JsValue>>;

    #[wasm_bindgen(method, getter, js_name = falsePath)]
    pub fn false_path(this: &JsCondition) -> Option<Vec<JsValue>>;
}

#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct JsWriterInfo {
    pub lineNumber: u32,
    pub columnNumber: u32,
    pub indentLevel: u16,
    pub lineStartIndentLevel: u16,
    pub lineStartColumnNumber: u32,
}

#[wasm_bindgen]
pub struct JsConditionResolverContext {
    #[wasm_bindgen(skip)]
    pub context: &'static mut ConditionResolverContext<'static, JsString, JsInfo, JsCondition>,
    /// Gets the writer info at the condition's location.
    pub writerInfo: JsWriterInfo,
}

#[derive(Clone)]
pub struct JsString {
    pub reference: js_sys::JsString
}

#[wasm_bindgen]
impl JsConditionResolverContext {
    /// Gets if a condition was true, false, or returns undefined when not yet resolved.
    pub fn getResolvedCondition(&mut self, condition: &JsCondition) -> Option<bool> {
        self.context.get_resolved_condition(condition)
    }

    /// Gets the writer info at a specified info or returns undefined when not yet resolved.
    pub fn getResolvedInfo(&mut self, info: &JsInfo) -> Option<JsWriterInfo> {
        let result = self.context.get_resolved_info(info);
        result.map(|x| JsWriterInfo {
            lineNumber: x.line_number,
            columnNumber: x.column_number,
            indentLevel: x.indent_level,
            lineStartIndentLevel: x.line_start_indent_level,
            lineStartColumnNumber: x.line_start_column_number,
        })
    }
}

impl InfoRef for JsInfo {
    fn get_unique_id(&self) -> usize {
        self.id()
    }

    fn get_name(&self) -> &'static str {
        ""
    }
}

impl StringRef for JsString {
    fn get_length(&self) -> usize {
        self.reference.length() as usize
    }

    fn get_text(self) -> String {
        self.reference.into()
    }

    fn get_text_clone(&self) -> String {
        self.reference.clone().into()
    }
}

impl ConditionRef<JsString, JsInfo, JsCondition> for JsCondition {
    fn get_unique_id(&self) -> usize {
        self.id()
    }

    fn get_name(&self) -> &'static str {
        ""
    }

    fn resolve(&self, context: &mut ConditionResolverContext<JsString, JsInfo, JsCondition>) -> Option<bool> {
        unsafe {
            // Force the object's lifetime to be 'static.
            // This is unsafe, but wasm_bindgen can't deal with lifetimes, so we'll just tell it we know better.
            let static_context = std::mem::transmute::<&mut ConditionResolverContext<JsString, JsInfo, JsCondition>, &mut ConditionResolverContext<'static, JsString, JsInfo, JsCondition>>(context);

            let writer_info = JsWriterInfo {
                lineNumber: context.writer_info.line_number,
                columnNumber: context.writer_info.column_number,
                indentLevel: context.writer_info.indent_level,
                lineStartIndentLevel: context.writer_info.line_start_indent_level,
                lineStartColumnNumber: context.writer_info.line_start_column_number,
            };
            let item = JsConditionResolverContext {
                context: static_context,
                writerInfo: writer_info,
            };

            self.condition(item)
        }
    }

    fn get_true_path(&self) -> Option<Rc<Vec<PrintItem<JsString, JsInfo, JsCondition>>>> {
        self.true_path().map(|items| Rc::new(get_rust_print_items(items)))
    }

    fn get_false_path(&self) -> Option<Rc<Vec<PrintItem<JsString, JsInfo, JsCondition>>>> {
        self.false_path().map(|items| Rc::new(get_rust_print_items(items)))
    }
}

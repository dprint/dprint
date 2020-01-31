use wasm_bindgen::prelude::*;
use wasm_bindgen::*;
use dprint_core::*;
use std::cell::RefCell;
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
    #[wasm_bindgen(js_name = RawJsCondition)]
    pub type RawJsCondition;

    #[wasm_bindgen(method, getter)]
    pub fn name(this: &RawJsCondition) -> js_sys::JsString;

    #[wasm_bindgen(method, getter)]
    pub fn id(this: &RawJsCondition) -> usize;

    #[wasm_bindgen(method)]
    pub fn condition(this: &RawJsCondition, context: JsConditionResolverContext) -> Option<bool>;

    #[wasm_bindgen(method, getter, js_name = truePath)]
    pub fn true_path(this: &RawJsCondition) -> Option<Vec<JsValue>>;

    #[wasm_bindgen(method, getter, js_name = falsePath)]
    pub fn false_path(this: &RawJsCondition) -> Option<Vec<JsValue>>;
}

#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct JsWriterInfo {
    pub lineNumber: u32,
    pub columnNumber: u32,
    pub indentLevel: u8,
    pub lineStartIndentLevel: u8,
    pub lineStartColumnNumber: u32,
}

#[wasm_bindgen]
pub struct JsConditionResolverContext {
    #[wasm_bindgen(skip)]
    pub context: &'static mut ConditionResolverContext<'static, JsString, JsInfo, JsCondition>,
    /// Gets the writer info at the condition's location.
    pub writerInfo: JsWriterInfo,
}

pub struct JsCondition {
    condition: RawJsCondition,
    cached_true_path: RefCell<Option<Option<PrintItemPath<JsString, JsInfo, JsCondition>>>>,
    cached_false_path: RefCell<Option<Option<PrintItemPath<JsString, JsInfo, JsCondition>>>>,
}

impl JsCondition {
    pub fn new(condition: RawJsCondition) -> JsCondition {
        JsCondition {
            condition,
            cached_true_path: RefCell::new(None),
            cached_false_path: RefCell::new(None),
        }
    }
}

#[derive(Clone)]
pub struct JsString {
    pub reference: js_sys::JsString,
    cached_value: RefCell<Option<String>>,
}

impl JsString {
    pub fn new(reference: js_sys::JsString) -> JsString {
        JsString {
            reference,
            cached_value: RefCell::new(None),
        }
    }
}

#[wasm_bindgen]
impl JsConditionResolverContext {
    /// Gets if a condition was true, false, or returns undefined when not yet resolved.
    pub fn getResolvedCondition(&mut self, condition: &RawJsCondition) -> Option<bool> {
        let reference = ConditionReference::new("", condition.id());
        self.context.get_resolved_condition(&reference)
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

impl InfoTrait for JsInfo {
    fn get_unique_id(&self) -> usize {
        self.id()
    }

    fn get_name(&self) -> &'static str {
        ""
    }
}

impl StringTrait for JsString {
    fn get_length(&self) -> usize {
        self.reference.length() as usize
    }

    fn get_text<'a>(&'a self) -> &'a str {
        if self.cached_value.borrow().is_none() {
            self.cached_value.borrow_mut().replace(self.reference.clone().into());
        }
        let cached_value = self.cached_value.borrow();
        let value = cached_value.as_ref().unwrap();

        // say the lifetime is the lifetime of the object (which is true)
        unsafe { std::mem::transmute::<&str, &'a str>(value) }
    }
}

impl ConditionTrait<JsString, JsInfo, JsCondition> for JsCondition {
    fn get_unique_id(&self) -> usize {
        self.condition.id()
    }

    fn get_name(&self) -> &'static str {
        ""
    }

    fn get_is_stored(&self) -> bool {
        // Store all JS conditions for now. A performance improvement in the future
        // would be to not store all these, but it's a very minor improvement.
        true
    }

    fn resolve(&self, context: &mut ConditionResolverContext<JsString, JsInfo, JsCondition>) -> Option<bool> {
        // Force the object's lifetime to be 'static.
        // This is unsafe, but wasm_bindgen can't deal with lifetimes, so we'll just tell it we know better.
        let static_context = unsafe { std::mem::transmute::<&mut ConditionResolverContext<JsString, JsInfo, JsCondition>, &mut ConditionResolverContext<'static, JsString, JsInfo, JsCondition>>(context) };

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

        self.condition.condition(item)
    }

    fn get_true_path(&self) -> Option<PrintItemPath<JsString, JsInfo, JsCondition>> {
        if self.cached_true_path.borrow().is_none() {
            let true_path = self.condition.true_path().map(|items| get_rust_print_items(items).into_rc_path());
            self.cached_true_path.replace(true_path);
        }

        self.cached_true_path.borrow().as_ref().map(|x| x.clone()).unwrap_or(None)
    }

    fn get_false_path(&self) -> Option<PrintItemPath<JsString, JsInfo, JsCondition>> {
        if self.cached_false_path.borrow().is_none() {
            let false_path = self.condition.false_path().map(|items| get_rust_print_items(items).into_rc_path());
            self.cached_false_path.replace(false_path);
        }

        self.cached_false_path.borrow().as_ref().map(|x| x.clone()).unwrap_or(None)
    }

    fn get_dependent_infos<'a>(&'a self) -> &'a Option<Vec<JsInfo>> {
        &None
    }
}

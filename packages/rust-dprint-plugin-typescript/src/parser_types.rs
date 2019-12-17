use std::str;
use super::*;
use std::collections::HashSet;
use swc_common::{SpanData, BytePos, comments::{Comments}};
use swc_ecma_ast::{Bool, JSXText, Null, Number, Regex, Str, Module};

pub struct Context {
    pub config: TypeScriptConfiguration,
    pub comments: Comments,
    pub file_bytes: Vec<u8>,
    pub current_node: Node,
    pub parent_stack: Vec<Node>,
    pub handled_comments: HashSet<BytePos>,
}

impl Context {
    pub fn get_span_text(self: &Context, span_data: &SpanData) -> &str {
        let bytes = &self.file_bytes[(span_data.lo.0 as usize)..(span_data.hi.0 as usize)];
        str::from_utf8(&bytes).unwrap()
    }

    pub fn parent(self: &Context) -> &Node {
        self.parent_stack.last().unwrap()
    }

    pub fn has_handled_comment(self: &Context, span_data: &SpanData) -> bool {
        self.handled_comments.contains(&span_data.lo)
    }

    pub fn mark_comment_handled(self: &mut Context, span_data: &SpanData) {
        self.handled_comments.insert(span_data.lo);
    }
}

#[derive(Clone)]
pub enum Node {
    /* module */
    Module(Module),
    /* literals */
    Bool(Bool),
    JsxText(JSXText),
    Null(Null),
    Num(Number),
    Regex(Regex),
    Str(Str),
}

use std::str;
use super::*;
use std::collections::HashSet;
use swc_common::{SpanData, BytePos, comments::{Comments, Comment}, SourceFile};
use swc_ecma_ast::{Bool, JSXText, Null, Number, Regex, Str, Module};

pub struct Context {
    pub config: TypeScriptConfiguration,
    comments: Comments,
    file_bytes: Vec<u8>,
    pub current_node: Node,
    pub parent_stack: Vec<Node>,
    handled_comments: HashSet<BytePos>,
    info: SourceFile,
}

impl Context {
    pub fn new(config: TypeScriptConfiguration, comments: Comments, file_bytes: Vec<u8>, current_node: Node, info: SourceFile) -> Context {
        Context {
            config,
            comments,
            file_bytes,
            current_node,
            parent_stack: Vec::new(),
            handled_comments: HashSet::new(),
            info,
        }
    }

    pub fn get_span_text(self: &Context, span_data: &SpanData) -> &str {
        let bytes = &self.file_bytes[(span_data.lo.0 as usize)..(span_data.hi.0 as usize)];
        str::from_utf8(&bytes).unwrap()
    }

    pub fn get_line_start(self: &Context, span_data: &SpanData) -> usize {
        self.info.lookup_line(span_data.lo).unwrap() + 1
    }

    pub fn get_line_end(self: &Context, span_data: &SpanData) -> usize {
        self.info.lookup_line(span_data.hi).unwrap() + 1
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

    pub fn get_leading_comments(self: &Context, span_data: &SpanData) -> Option<Vec<Comment>> {
        self.comments.leading_comments(span_data.lo).map(|c| c.clone())
    }

    pub fn get_trailing_comments(self: &Context, span_data: &SpanData) -> Option<Vec<Comment>> {
        self.comments.trailing_comments(span_data.hi).map(|c| c.clone())
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

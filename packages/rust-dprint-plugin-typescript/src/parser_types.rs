use std::str;
use std::rc::Rc;
use super::*;
use std::collections::HashSet;
use swc_common::{SpanData, BytePos, comments::{Comments, Comment}, SourceFile, Spanned, Span};
use swc_ecma_ast::{BigInt, Bool, CallExpr, JSXText, Null, Number, Regex, Str, Module, ExprStmt, TsTypeParamInstantiation, ModuleItem, Stmt, Expr, ExprOrSuper, Lit,
    ExprOrSpread};

pub struct Context {
    pub config: TypeScriptConfiguration,
    comments: Rc<Comments>,
    file_bytes: Rc<Vec<u8>>,
    pub current_node: Node,
    pub parent_stack: Vec<Node>,
    handled_comments: HashSet<BytePos>,
    info: Rc<SourceFile>,
}

impl Context {
    pub fn new(config: TypeScriptConfiguration, comments: Comments, file_bytes: Vec<u8>, current_node: Node, info: SourceFile) -> Context {
        Context {
            config,
            comments: Rc::new(comments),
            file_bytes: Rc::new(file_bytes),
            current_node,
            parent_stack: Vec::new(),
            handled_comments: HashSet::new(),
            info: Rc::new(info),
        }
    }

    pub fn get_text_range(self: &Context, spanned: &impl Spanned) -> TextRange {
        TextRange::new(self.comments.clone(), self.info.clone(), self.file_bytes.clone(), spanned.span().data())
    }

    pub fn parent(self: &Context) -> &Node {
        self.parent_stack.last().unwrap()
    }

    pub fn has_handled_comment(self: &Context, comment: &TextRange) -> bool {
        self.handled_comments.contains(&comment.lo())
    }

    pub fn mark_comment_handled(self: &mut Context, comment: &TextRange) {
        self.handled_comments.insert(comment.lo());
    }
}

#[derive(Clone)]
pub struct TextRange {
    comments: Rc<Comments>,
    info: Rc<SourceFile>,
    file_bytes: Rc<Vec<u8>>,
    data: SpanData,
    line_start: Option<usize>,
    line_end: Option<usize>,
}

impl TextRange {
    pub fn new(comments: Rc<Comments>, info: Rc<SourceFile>, file_bytes: Rc<Vec<u8>>, data: SpanData) -> TextRange {
        TextRange {
            comments,
            info,
            file_bytes,
            data,
            line_start: Option::None,
            line_end: Option::None,
        }
    }

    pub fn lo(self: &TextRange) -> BytePos {
        self.data.lo
    }

    pub fn hi(self: &TextRange) -> BytePos {
        self.data.hi
    }

    pub fn leading_comments(self: &TextRange) -> Vec<Comment> {
        self.comments.leading_comments(self.data.lo).map(|c| c.clone()).unwrap_or_default()
    }

    pub fn trailing_comments(self: &TextRange) -> Vec<Comment> {
        self.comments.trailing_comments(self.data.hi).map(|c| c.clone()).unwrap_or_default()
    }

    pub fn line_start(self: &mut TextRange) -> usize {
        if self.line_start.is_none() {
            self.line_start = Some(self.info.lookup_line(self.data.lo).unwrap() + 1);
        }
        self.line_start.unwrap()
    }

    pub fn line_end(self: &mut TextRange) -> usize {
        if self.line_end.is_none() {
            self.line_end = Some(self.info.lookup_line(self.data.hi).unwrap() + 1);
        }
        self.line_end.unwrap()
    }

    pub fn text(self: &TextRange) -> &str {
        let bytes = &self.file_bytes[(self.data.lo.0 as usize)..(self.data.hi.0 as usize)];
        str::from_utf8(&bytes).unwrap()
    }
}

#[derive(Clone)]
pub enum Node {
    /* expressions */
    CallExpr(CallExpr),
    ExprOrSpread(ExprOrSpread),
    /* literals */
    BigInt(BigInt),
    Bool(Bool),
    JsxText(JSXText),
    Null(Null),
    Num(Number),
    Regex(Regex),
    Str(Str),
    /* module */
    Module(Module),
    /* statements */
    ExprStmt(ExprStmt),
    /* types */
    TsTypeParamInstantiation(TsTypeParamInstantiation),
    /* unknown */
    Unknown(Span),
}

/* Into node implementations */

impl From<ModuleItem> for Node {
    fn from(item: ModuleItem) -> Node {
        match item {
            ModuleItem::Stmt(node) => node.into(),
            _ => Node::Unknown(item.span()), // todo: implement others
        }
    }
}

impl From<Stmt> for Node {
    fn from(stmt: Stmt) -> Node {
        match stmt {
            Stmt::Expr(node) => Node::ExprStmt(node),
            _ => Node::Unknown(stmt.span()), // todo: implement others
        }
    }
}

impl From<Expr> for Node {
    fn from(expr: Expr) -> Node {
        match expr {
            Expr::Lit(lit) => lit.into(),
            _ => Node::Unknown(expr.span()), // todo: implement others
        }
    }
}

impl From<ExprOrSuper> for Node {
    fn from(expr_or_super: ExprOrSuper) -> Node {
        match expr_or_super {
            ExprOrSuper::Expr(box expr) => expr.into(),
            _ => Node::Unknown(expr_or_super.span()), // todo: implement others
        }
    }
}

impl From<Lit> for Node {
    fn from(lit: Lit) -> Node {
        match lit {
            Lit::BigInt(node) => node.into(),
            Lit::Bool(node) => node.into(),
            Lit::JSXText(node) => node.into(),
            Lit::Null(node) => node.into(),
            Lit::Num(node) => node.into(),
            Lit::Regex(node) => node.into(),
            Lit::Str(node) => node.into(),
        }
    }
}

impl From<BigInt> for Node {
    fn from(node: BigInt) -> Node {
        Node::BigInt(node)
    }
}

impl From<Bool> for Node {
    fn from(node: Bool) -> Node {
        Node::Bool(node)
    }
}

impl From<JSXText> for Node {
    fn from(node: JSXText) -> Node {
        Node::JsxText(node)
    }
}

impl From<Null> for Node {
    fn from(node: Null) -> Node {
        Node::Null(node)
    }
}

impl From<Number> for Node {
    fn from(node: Number) -> Node {
        Node::Num(node)
    }
}

impl From<Regex> for Node {
    fn from(node: Regex) -> Node {
        Node::Regex(node)
    }
}

impl From<Str> for Node {
    fn from(node: Str) -> Node {
        Node::Str(node)
    }
}

impl From<ExprOrSpread> for Node {
    fn from(node: ExprOrSpread) -> Node {
        Node::ExprOrSpread(node)
    }
}

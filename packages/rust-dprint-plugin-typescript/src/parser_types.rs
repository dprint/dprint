use std::str;
use std::rc::Rc;
use super::*;
use std::collections::HashSet;
use swc_common::{SpanData, BytePos, comments::{Comments, Comment}, SourceFile, Spanned, Span};
use swc_ecma_ast::{BigInt, Bool, CallExpr, Ident, JSXText, Null, Number, Regex, Str, Module, ExprStmt, TsTypeAnn, TsTypeParamInstantiation,
    ModuleItem, Stmt, Expr, ExprOrSuper, Lit, ExprOrSpread, FnExpr, ArrowExpr};
use swc_ecma_parser::{token::{Token, TokenAndSpan}};

pub struct Context {
    pub config: TypeScriptConfiguration,
    comments: Rc<Comments>,
    tokens: Rc<Vec<TokenAndSpan>>,
    file_bytes: Rc<Vec<u8>>,
    pub current_node: Node,
    pub parent_stack: Vec<Node>,
    handled_comments: HashSet<BytePos>,
    info: Rc<SourceFile>,
}

impl Context {
    pub fn new(
        config: TypeScriptConfiguration,
        comments: Comments,
        tokens: Vec<TokenAndSpan>,
        file_bytes: Vec<u8>,
        current_node: Node,
        info: SourceFile
    ) -> Context {
        Context {
            config,
            comments: Rc::new(comments),
            tokens: Rc::new(tokens),
            file_bytes: Rc::new(file_bytes),
            current_node,
            parent_stack: Vec::new(),
            handled_comments: HashSet::new(),
            info: Rc::new(info),
        }
    }

    pub fn get_text_range(&self, spanned: &impl Spanned) -> TextRange {
        TextRange::new(self.comments.clone(), self.info.clone(), self.file_bytes.clone(), spanned.span().data())
    }

    pub fn parent(&self) -> &Node {
        self.parent_stack.last().unwrap()
    }

    pub fn has_handled_comment(&self, comment: &TextRange) -> bool {
        self.handled_comments.contains(&comment.lo())
    }

    pub fn mark_comment_handled(&mut self, comment: &TextRange) {
        self.handled_comments.insert(comment.lo());
    }

    pub fn get_first_open_paren_token_before(&self, range: &TextRange) -> Option<TokenAndSpan> {
        let pos = range.lo();
        let mut found_token = Option::None;
        for token in self.tokens.iter() {
            if token.span.data().lo >= pos {
                break;
            }
            if token.token == Token::LParen {
                found_token = Some(token);
            }
        }
        found_token.map(|x| x.to_owned())
    }
}

#[derive(Clone)]
pub struct TextRange {
    comments: Rc<Comments>,
    info: Rc<SourceFile>,
    file_bytes: Rc<Vec<u8>>,
    data: SpanData,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

impl TextRange {
    pub fn new(comments: Rc<Comments>, info: Rc<SourceFile>, file_bytes: Rc<Vec<u8>>, data: SpanData) -> TextRange {
        TextRange {
            comments,
            info,
            file_bytes,
            data,
            start_line: Option::None,
            end_line: Option::None,
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

    pub fn start_line(self: &mut TextRange) -> usize {
        if self.start_line.is_none() {
            self.start_line = Some(self.info.lookup_line(self.data.lo).unwrap() + 1);
        }
        self.start_line.unwrap()
    }

    pub fn end_line(self: &mut TextRange) -> usize {
        if self.end_line.is_none() {
            self.end_line = Some(self.info.lookup_line(self.data.hi).unwrap() + 1);
        }
        self.end_line.unwrap()
    }

    pub fn text(self: &TextRange) -> &str {
        let bytes = &self.file_bytes[(self.data.lo.0 as usize)..(self.data.hi.0 as usize)];
        str::from_utf8(&bytes).unwrap()
    }
}

pub trait NodeKinded {
    fn kind(&self) -> NodeKind;
}

macro_rules! generate_node {
    ($($node_name:ident),*) => {
        #[derive(Clone, PartialEq)]
        pub enum NodeKind {
            $($node_name),*
        }

        #[derive(Clone)]
        pub enum Node {
            $($node_name($node_name)),*
        }

        impl NodeKinded for Node {
            fn kind(&self) -> NodeKind {
                match self {
                    $(Node::$node_name(_) => NodeKind::$node_name),*
                }
            }
        }

        $(
        impl NodeKinded for $node_name {
            fn kind(&self) -> NodeKind {
                NodeKind::$node_name
            }
        }
        )*

        $(
        impl From<$node_name> for Node {
            fn from(node: $node_name) -> Node {
                Node::$node_name(node)
            }
        }
        )*

        impl Spanned for Node {
            fn span(&self) -> Span {
                match self {
                    $(Node::$node_name(node) => node.span()),*
                }
            }
        }
    };
}

pub type Unknown = Span;

generate_node! [
    /* common */
    Ident,
    /* expressions */
    CallExpr,
    ExprOrSpread,
    FnExpr,
    ArrowExpr,
    /* literals */
    BigInt,
    Bool,
    JSXText,
    Null,
    Number,
    Regex,
    Str,
    /* module */
    Module,
    /* statements */
    ExprStmt,
    /* types */
    TsTypeAnn,
    TsTypeParamInstantiation,
    /* unknown */
    Unknown
];

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
            Expr::Lit(node) => node.into(),
            Expr::Arrow(node) => node.into(),
            Expr::Fn(node) => node.into(),
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

/* NodeKinded implementations */

impl NodeKinded for Stmt {
    fn kind(&self) -> NodeKind {
        match self {
            Stmt::Expr(node) => node.kind(),
            _ => NodeKind::Unknown,
        }
    }
}

impl NodeKinded for Expr {
    fn kind(&self) -> NodeKind {
        match self {
            Expr::Lit(node) => node.kind(),
            Expr::Fn(node) => node.kind(),
            Expr::Arrow(node) => node.kind(),
            _ => NodeKind::Unknown,
        }
    }
}

impl NodeKinded for ExprOrSuper {
    fn kind(&self) -> NodeKind {
        match self {
            ExprOrSuper::Expr(node) => node.kind(),
            _ => NodeKind::Unknown,
        }
    }
}

impl NodeKinded for Lit {
    fn kind(&self) -> NodeKind {
        match self {
            Lit::BigInt(node) => node.kind(),
            Lit::Bool(node) => node.kind(),
            Lit::JSXText(node) => node.kind(),
            Lit::Null(node) => node.kind(),
            Lit::Num(node) => node.kind(),
            Lit::Regex(node) => node.kind(),
            Lit::Str(node) => node.kind(),
        }
    }
}

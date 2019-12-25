use std::str;
use std::rc::Rc;
use super::*;
use std::collections::HashSet;
use swc_common::{SpanData, BytePos, comments::{Comments, Comment}, SourceFile, Spanned, Span};
use swc_ecma_ast::{BigInt, Bool, CallExpr, Ident, JSXText, Null, Number, Regex, Str, Module, ExprStmt, TsType, TsTypeAnn, TsTypeParamInstantiation,
    ModuleItem, Stmt, Expr, ExprOrSuper, Lit, ExprOrSpread, FnExpr, ArrowExpr, BreakStmt, ContinueStmt, DebuggerStmt, EmptyStmt, TsExportAssignment, ModuleDecl,
    ArrayLit, ArrayPat, Pat};
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
        self.get_first_token_before(&range, Token::LParen)
    }

    pub fn get_first_angle_bracket_token_before(&self, range: &TextRange) -> Option<TokenAndSpan> {
        self.get_first_token_before(&range, Token::LBracket)
    }

    fn get_first_token_before(&self, range: &TextRange, searching_token: Token) -> Option<TokenAndSpan> {
        let pos = range.lo();
        let mut found_token = Option::None;
        for token in self.tokens.iter() {
            if token.span.data().lo >= pos {
                break;
            }
            if token.token == searching_token {
                found_token = Some(token);
            }
        }
        found_token.map(|x| x.to_owned())
    }

    pub fn get_first_open_bracket_token_within(&self, range: &TextRange) -> Option<TokenAndSpan> {
        self.get_first_token_within(&range, Token::LBracket)
    }

    fn get_first_token_within(&self, range: &TextRange, searching_token: Token) -> Option<TokenAndSpan> {
        let pos = range.lo();
        let end = range.hi();
        let mut found_token = Option::None;
        for token in self.tokens.iter() {
            let token_pos = token.span.data().lo;
            if token_pos >= end {
                break;
            } else if token_pos >= pos && token.token == searching_token {
                found_token = Some(token);
            }
        }
        found_token.map(|x| x.to_owned())
    }

    pub fn get_first_comma_after(&self, range: &TextRange) -> Option<TokenAndSpan> {
        self.get_first_token_after(&range, Token::Comma)
    }

    fn get_first_token_after(&self, range: &TextRange, searching_token: Token) -> Option<TokenAndSpan> {
        let pos = range.lo();
        let end = range.hi();
        for token in self.tokens.iter() {
            let token_pos = token.span.data().lo;
            if token_pos >= end {
                break;
            } else if token_pos >= pos {
                if token.token == searching_token {
                    return Some(token.to_owned());
                }
            }
        }

        Option::None
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
        #[derive(Clone, PartialEq, Debug)]
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
    ArrayLit,
    ArrowExpr,
    CallExpr,
    ExprOrSpread,
    FnExpr,
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
    /* patterns */
    ArrayPat,
    /* statements */
    BreakStmt,
    ContinueStmt,
    DebuggerStmt,
    EmptyStmt,
    TsExportAssignment,
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
            ModuleItem::ModuleDecl(node) => node.into(),
        }
    }
}

impl From<Stmt> for Node {
    fn from(stmt: Stmt) -> Node {
        match stmt {
            Stmt::Break(node) => Node::BreakStmt(node),
            Stmt::Continue(node) => Node::ContinueStmt(node),
            Stmt::Debugger(node) => Node::DebuggerStmt(node),
            Stmt::Empty(node) => Node::EmptyStmt(node),
            Stmt::Expr(node) => Node::ExprStmt(node),
            _ => Node::Unknown(stmt.span()), // todo: implement others
        }
    }
}

impl From<Expr> for Node {
    fn from(expr: Expr) -> Node {
        match expr {
            Expr::Array(node) => node.into(),
            Expr::Arrow(node) => node.into(),
            Expr::Call(node) => node.into(),
            Expr::Fn(node) => node.into(),
            Expr::Ident(node) => node.into(),
            Expr::Lit(node) => node.into(),
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

impl From<ModuleDecl> for Node {
    fn from(dec: ModuleDecl) -> Node {
        match dec {
            ModuleDecl::TsExportAssignment(node) => node.into(),
            _ => Node::Unknown(dec.span()), // todo: implement others
        }
    }
}

impl From<Pat> for Node {
    fn from(pat: Pat) -> Node {
        match pat {
            Pat::Array(node) => node.into(),
            _ => Node::Unknown(pat.span()), // todo: implement others
        }
    }
}

impl From<TsType> for Node {
    fn from(ts_type: TsType) -> Node {
        match ts_type {
            _ => Node::Unknown(ts_type.span()), // todo: implement others
        }
    }
}

/* NodeKinded implementations */

impl NodeKinded for ModuleItem {
    fn kind(&self) -> NodeKind {
        match self {
            ModuleItem::Stmt(node) => node.kind(),
            ModuleItem::ModuleDecl(node) => node.kind(),
        }
    }
}

impl NodeKinded for Stmt {
    fn kind(&self) -> NodeKind {
        match self {
            Stmt::Break(node) => node.kind(),
            Stmt::Continue(node) => node.kind(),
            Stmt::Debugger(node) => node.kind(),
            Stmt::Empty(node) => node.kind(),
            Stmt::Expr(node) => node.kind(),
            _ => NodeKind::Unknown,
        }
    }
}

impl NodeKinded for Expr {
    fn kind(&self) -> NodeKind {
        match self {
            Expr::Arrow(node) => node.kind(),
            Expr::Call(node) => node.kind(),
            Expr::Fn(node) => node.kind(),
            Expr::Ident(node) => node.kind(),
            Expr::Lit(node) => node.kind(),
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

impl NodeKinded for ModuleDecl {
    fn kind(&self) -> NodeKind {
        match self {
            ModuleDecl::TsExportAssignment(node) => node.kind(),
            _ => NodeKind::Unknown, // todo: implement others
        }
    }
}

impl NodeKinded for Pat {
    fn kind(&self) -> NodeKind {
        match self {
            Pat::Array(node) => node.kind(),
            _ => NodeKind::Unknown, // todo: implement others
        }
    }
}

impl NodeKinded for TsType {
    fn kind(&self) -> NodeKind {
        match self {
            _ => NodeKind::Unknown, // todo: implement others
        }
    }
}

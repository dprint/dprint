use std::str;
use std::rc::Rc;
use super::*;
use std::collections::HashSet;
use swc_common::{SpanData, BytePos, comments::{Comments, Comment}, SourceFile, Spanned, Span};
use swc_ecma_ast::{BigInt, Bool, CallExpr, Ident, JSXText, Null, Number, Regex, Str, Module, ExprStmt, TsType, TsTypeAnn, TsTypeParamInstantiation,
    ModuleItem, Stmt, Expr, ExprOrSuper, Lit, ExprOrSpread, FnExpr, ArrowExpr, BreakStmt, ContinueStmt, DebuggerStmt, EmptyStmt, TsExportAssignment, ModuleDecl,
    ArrayLit, ArrayPat, Pat, VarDecl, VarDeclarator, Decl, ExportAll, TsEnumDecl, TsEnumMember, TsEnumMemberId, TsTypeAliasDecl, TsTypeParamDecl, TsTypeParam,
    TsLitType, TsLit};
use swc_ecma_parser::{token::{Token, TokenAndSpan}};

pub struct Context {
    pub config: TypeScriptConfiguration,
    pub comments: Rc<Comments>,
    tokens: Rc<Vec<TokenAndSpan>>,
    file_bytes: Rc<Vec<u8>>,
    pub current_node: Node,
    pub parent_stack: Vec<Node>,
    handled_comments: HashSet<BytePos>,
    pub info: Rc<SourceFile>,
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

    pub fn parent(&self) -> &Node {
        self.parent_stack.last().unwrap()
    }

    pub fn has_handled_comment(&self, comment: &Comment) -> bool {
        self.handled_comments.contains(&comment.lo())
    }

    pub fn mark_comment_handled(&mut self, comment: &Comment) {
        self.handled_comments.insert(comment.lo());
    }

    pub fn get_text(&self, span_data: &SpanData) -> &str {
        let bytes = &self.file_bytes[(span_data.lo.0 as usize)..(span_data.hi.0 as usize)];
        str::from_utf8(&bytes).unwrap()
    }

    pub fn get_first_open_paren_token_before(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before(node, Token::LParen)
    }

    pub fn get_first_angle_bracket_token_before(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before(node, Token::LBracket)
    }

    fn get_first_token_before(&self, node: &dyn Ranged, searching_token: Token) -> Option<TokenAndSpan> {
        let pos = node.lo();
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

    pub fn get_first_open_brace_token_within(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, Token::LBrace)
    }

    pub fn get_first_open_bracket_token_within(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, Token::LBracket)
    }

    fn get_first_token_within(&self, node: &dyn Ranged, searching_token: Token) -> Option<TokenAndSpan> {
        let node_span_data = node.span().data();
        let pos = node_span_data.lo;
        let end = node_span_data.hi;
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

    pub fn get_first_comma_after(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_after(node, Token::Comma)
    }

    fn get_first_token_after(&self, node: &dyn Ranged, searching_token: Token) -> Option<TokenAndSpan> {
        let node_span_data = node.span().data();
        let pos = node_span_data.lo;
        let end = node_span_data.hi;
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

pub trait NodeKinded {
    fn kind(&self) -> NodeKind;
}

pub trait Ranged : Spanned {
    fn lo(&self) -> BytePos;
    fn hi(&self) -> BytePos;
    fn start_line(&self, context: &mut Context) -> usize;
    fn end_line(&self, context: &mut Context) -> usize;
    fn text<'a>(&self, context: &'a Context) -> &'a str;
    fn leading_comments(&self, context: &Context) -> Vec<Comment>;
    fn trailing_comments(&self, context: &Context) -> Vec<Comment>;
}

impl<T> Ranged for T where T : Spanned {
    fn lo(&self) -> BytePos {
        self.span().data().lo
    }

    fn hi(&self) -> BytePos {
        self.span().data().hi
    }

    fn start_line(&self, context: &mut Context) -> usize {
        context.info.lookup_line(self.lo()).unwrap() + 1
    }

    fn end_line(&self, context: &mut Context) -> usize {
        context.info.lookup_line(self.hi()).unwrap() + 1
    }

    fn text<'a>(&self, context: &'a Context) -> &'a str {
        let span_data = self.span().data();
        context.get_text(&span_data)
    }

    fn leading_comments(&self, context: &Context) -> Vec<Comment> {
        context.comments.leading_comments(self.lo()).map(|c| c.clone()).unwrap_or_default()
    }

    fn trailing_comments(&self, context: &Context) -> Vec<Comment> {
        context.comments.trailing_comments(self.hi()).map(|c| c.clone()).unwrap_or_default()
    }
}

pub trait NodeLike : NodeKinded + Spanned + Ranged {
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
    /* declarations */
    TsEnumDecl,
    TsEnumMember,
    TsTypeAliasDecl,
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
    ExportAll,
    ExprStmt,
    TsExportAssignment,
    VarDecl,
    VarDeclarator,
    /* types */
    TsLitType,
    TsTypeAnn,
    TsTypeParamInstantiation,
    TsTypeParamDecl,
    TsTypeParam,
    /* unknown */
    TokenAndSpan,
    Comment,
    Unknown
];

/* custom enums */

pub enum TypeParamNode {
    Instantiation(TsTypeParamInstantiation),
    Decl(TsTypeParamDecl)
}

impl TypeParamNode {
    pub fn params(self) -> Vec<Node> {
        match self {
            TypeParamNode::Instantiation(node) => node.params.into_iter().map(|box p| p.into()).collect(),
            TypeParamNode::Decl(node) => node.params.into_iter().map(|p| p.into()).collect(),
        }
    }
}

/* fully implemented From and NodeKinded implementations */

macro_rules! generate_traits {
    ($enum_name:ident, $($member_name:ident),*) => {
        impl From<$enum_name> for Node {
            fn from(id: $enum_name) -> Node {
                match id {
                    $($enum_name::$member_name(node) => node.into()),*
                }
            }
        }

        impl NodeKinded for $enum_name {
            fn kind(&self) -> NodeKind {
                match self {
                    $($enum_name::$member_name(node) => node.kind()),*
                }
            }
        }
    };
}

generate_traits![Lit, BigInt, Bool, JSXText, Null, Num, Regex, Str];
generate_traits![ModuleItem, Stmt, ModuleDecl];
generate_traits![TsEnumMemberId, Ident, Str];
generate_traits![TypeParamNode, Instantiation, Decl];
generate_traits![TsLit, Number, Str, Bool];

/* temporary manual from implementations */

impl From<Decl> for Node {
    fn from(decl: Decl) -> Node {
        match decl {
            Decl::TsEnum(node) => node.into(),
            Decl::TsTypeAlias(node) => node.into(),
            Decl::Var(node) => node.into(),
            _ => Node::Unknown(decl.span()), // todo: implement others
        }
    }
}

impl From<Stmt> for Node {
    fn from(stmt: Stmt) -> Node {
        match stmt {
            Stmt::Break(node) => node.into(),
            Stmt::Continue(node) => node.into(),
            Stmt::Debugger(node) => node.into(),
            Stmt::Decl(node) => node.into(),
            Stmt::Empty(node) => node.into(),
            Stmt::Expr(node) => node.into(),
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

impl From<ModuleDecl> for Node {
    fn from(dec: ModuleDecl) -> Node {
        match dec {
            ModuleDecl::TsExportAssignment(node) => node.into(),
            ModuleDecl::ExportAll(node) => node.into(),
            _ => Node::Unknown(dec.span()), // todo: implement others
        }
    }
}

impl From<Pat> for Node {
    fn from(pat: Pat) -> Node {
        match pat {
            Pat::Array(node) => node.into(),
            Pat::Ident(node) => node.into(),
            _ => Node::Unknown(pat.span()), // todo: implement others
        }
    }
}

impl From<TsType> for Node {
    fn from(ts_type: TsType) -> Node {
        match ts_type {
            TsType::TsLitType(node) => node.into(),
            _ => Node::Unknown(ts_type.span()), // todo: implement others
        }
    }
}

/* temporary manual NodeKinded implementations */

impl NodeKinded for Decl {
    fn kind(&self) -> NodeKind {
        match self {
            Decl::TsEnum(node) => node.kind(),
            Decl::TsTypeAlias(node) => node.kind(),
            Decl::Var(node) => node.kind(),
            _ => NodeKind::Unknown,
        }
    }
}

impl NodeKinded for Stmt {
    fn kind(&self) -> NodeKind {
        match self {
            Stmt::Break(node) => node.kind(),
            Stmt::Continue(node) => node.kind(),
            Stmt::Decl(node) => node.kind(),
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

impl NodeKinded for ModuleDecl {
    fn kind(&self) -> NodeKind {
        match self {
            ModuleDecl::TsExportAssignment(node) => node.kind(),
            ModuleDecl::ExportAll(node) => node.kind(),
            _ => NodeKind::Unknown, // todo: implement others
        }
    }
}

impl NodeKinded for Pat {
    fn kind(&self) -> NodeKind {
        match self {
            Pat::Array(node) => node.kind(),
            Pat::Ident(node) => node.kind(),
            _ => NodeKind::Unknown, // todo: implement others
        }
    }
}

impl NodeKinded for TsType {
    fn kind(&self) -> NodeKind {
        match self {
            TsType::TsLitType(node) => node.kind(),
            _ => NodeKind::Unknown, // todo: implement others
        }
    }
}

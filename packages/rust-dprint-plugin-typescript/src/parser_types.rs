use std::str;
use std::rc::Rc;
use super::*;
use std::collections::{HashSet, HashMap};
use dprint_core::{Info};
use utils::{Stack};
use swc_common::{SpanData, BytePos, comments::{Comments, Comment}, SourceFile, Spanned, Span};
use swc_ecma_ast::*;
use swc_ecma_parser::{token::{Token, TokenAndSpan, BinOpToken}};

pub struct Context {
    pub config: TypeScriptConfiguration,
    pub comments: Rc<Comments>,
    tokens: Rc<Vec<TokenAndSpan>>,
    pub file_bytes: Rc<Vec<u8>>,
    pub current_node: Node,
    pub parent_stack: Vec<Node>, // todo: use stack type
    handled_comments: HashSet<BytePos>,
    pub info: Rc<SourceFile>,
    stored_infos: HashMap<BytePos, Info>,
    pub end_statement_or_member_infos: Stack<Info>,
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
            stored_infos: HashMap::new(),
            end_statement_or_member_infos: Stack::new(),
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

    pub fn store_info_for_node(&mut self, node: &dyn Ranged, info: Info) {
        self.stored_infos.insert(node.lo(), info);
    }

    pub fn get_info_for_node(&self, node: &dyn Ranged) -> Option<&Info> {
        self.stored_infos.get(&node.lo())
    }

    pub fn get_token_at(&self, node: &dyn Ranged) -> TokenAndSpan {
        let pos = node.lo();
        for token in self.tokens.iter() {
            if token.span.data().lo == pos {
                return token.clone();
            }
        }
        panic!("Could not find expected token.");
    }

    pub fn get_first_open_paren_token_before(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before(node, Token::LParen)
    }

    pub fn get_first_angle_bracket_token_before(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before(node, Token::BinOp(BinOpToken::Lt))
    }

    pub fn get_first_open_brace_token_before(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_before(node, Token::LBrace)
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

    pub fn get_first_open_paren_token_within(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, Token::LParen)
    }

    pub fn get_first_open_brace_token_within(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, Token::LBrace)
    }

    pub fn get_first_open_bracket_token_within(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, Token::LBracket)
    }

    pub fn get_first_comma_within(&self, node: &dyn Ranged) -> Option<TokenAndSpan> {
        self.get_first_token_within(node, Token::Comma)
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

    pub fn get_first_token_after_with_text(&self, node: &dyn Ranged, searching_token_text: &str) -> Option<TokenAndSpan> {
        let node_span_data = node.span().data();
        let pos = node_span_data.hi;
        for token in self.tokens.iter() {
            let token_pos = token.span.data().lo;
            if token_pos >= pos && token.span.text(self) == searching_token_text {
                return Some(token.to_owned());
            }
        }

        Option::None
    }

    pub fn get_token_text_at_pos(&self, pos: BytePos) -> Option<&str> {
        for token in self.tokens.iter() {
            let token_pos = token.span.data().lo;
            if token_pos == pos {
                return Some(token.span.text(self));
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
    /* class */
    ClassMethod,
    ClassProp,
    Constructor,
    Decorator,
    PrivateMethod,
    PrivateProp,
    TsParamProp,
    /* common */
    ComputedPropName,
    Ident,
    Invalid,
    PrivateName,
    TsQualifiedName,
    /* declarations */
    ClassDecl,
    ExportDecl,
    ExportDefaultDecl,
    ExportDefaultExpr,
    FnDecl,
    Function,
    NamedExport,
    ImportDecl,
    TsEnumDecl,
    TsEnumMember,
    TsImportEqualsDecl,
    TsInterfaceDecl,
    TsTypeAliasDecl,
    TsModuleDecl,
    TsModuleBlock,
    TsNamespaceDecl,
    /* exports */
    DefaultExportSpecifier,
    NamespaceExportSpecifier,
    NamedExportSpecifier,
    /* expressions */
    ArrayLit,
    ArrowExpr,
    AssignExpr,
    AssignProp,
    AwaitExpr,
    BinExpr,
    CallExpr,
    ClassExpr,
    CondExpr,
    ExprOrSpread,
    FnExpr,
    GetterProp,
    JSXElement,
    JSXEmptyExpr,
    JSXFragment,
    JSXMemberExpr,
    JSXNamespacedName,
    KeyValueProp,
    MemberExpr,
    MetaPropExpr,
    MethodProp,
    NewExpr,
    ParenExpr,
    ObjectLit,
    OptChainExpr,
    SeqExpr,
    SetterProp,
    SpreadElement,
    Super,
    TaggedTpl,
    ThisExpr,
    Tpl,
    TsAsExpr,
    TsConstAssertion,
    TsTypeCastExpr,
    TsExprWithTypeArgs,
    TsNonNullExpr,
    TsTypeAssertion,
    UnaryExpr,
    UpdateExpr,
    YieldExpr,
    /* imports */
    ImportDefault,
    ImportSpecific,
    ImportStarAs,
    TsExternalModuleRef,
    /* interface / type element */
    TsInterfaceBody,
    TsCallSignatureDecl,
    TsConstructSignatureDecl,
    TsIndexSignature,
    TsMethodSignature,
    TsPropertySignature,
    TsTypeLit,
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
    AssignPat,
    AssignPatProp,
    KeyValuePatProp,
    ObjectPat,
    RestPat,
    /* statements */
    BlockStmt,
    BreakStmt,
    ContinueStmt,
    DebuggerStmt,
    DoWhileStmt,
    EmptyStmt,
    ExportAll,
    ExprStmt,
    ForStmt,
    ForInStmt,
    ForOfStmt,
    IfStmt,
    LabeledStmt,
    ReturnStmt,
    SwitchStmt,
    ThrowStmt,
    TryStmt,
    TsExportAssignment,
    TsNamespaceExportDecl,
    VarDecl,
    VarDeclarator,
    WithStmt,
    WhileStmt,
    /* types */
    TsArrayType,
    TsConditionalType,
    TsConstructorType,
    TsKeywordType,
    TsFnType,
    TsImportType,
    TsIndexedAccessType,
    TsInferType,
    TsIntersectionType,
    TsLitType,
    TsMappedType,
    TsOptionalType,
    TsParenthesizedType,
    TsRestType,
    TsThisType,
    TsTupleType,
    TsTypeAnn,
    TsTypeOperator,
    TsTypeParamInstantiation,
    TsTypeParamDecl,
    TsTypeParam,
    TsTypePredicate,
    TsTypeQuery,
    TsTypeRef,
    TsUnionType,
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

pub enum NamedImportOrExportDeclaration {
    Import(ImportDecl),
    Export(NamedExport),
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

generate_traits![BlockStmtOrExpr, BlockStmt, Expr];
generate_traits![ClassMember, Constructor, Method, PrivateMethod, ClassProp, PrivateProp, TsIndexSignature];
generate_traits![Decl, Class, Fn, Var, TsInterface, TsTypeAlias, TsEnum, TsModule];
generate_traits![Lit, BigInt, Bool, JSXText, Null, Num, Regex, Str];
generate_traits![ImportSpecifier, Specific, Default, Namespace];
generate_traits![ModuleItem, Stmt, ModuleDecl];
generate_traits![NamedImportOrExportDeclaration, Import, Export];
generate_traits![ObjectPatProp, KeyValue, Assign, Rest];
generate_traits![PatOrExpr, Pat, Expr];
generate_traits![TsEnumMemberId, Ident, Str];
generate_traits![TsLit, Number, Str, Bool];
generate_traits![TypeParamNode, Instantiation, Decl];
generate_traits![TsTypeElement, TsCallSignatureDecl, TsConstructSignatureDecl, TsPropertySignature, TsMethodSignature, TsIndexSignature];
generate_traits![TsFnParam, Ident, Array, Rest, Object];
generate_traits![Expr, This, Array, Object, Fn, Unary, Update, Bin, Assign, Member, Cond, Call, New, Seq, Ident, Lit, Tpl, TaggedTpl, Arrow,
    Class, Yield, MetaProp, Await, Paren, JSXMebmer, JSXNamespacedName, JSXEmpty, JSXElement, JSXFragment, TsTypeAssertion, TsConstAssertion,
    TsNonNull, TsTypeCast, TsAs, PrivateName, OptChain, Invalid];
generate_traits![PropOrSpread, Spread, Prop];
generate_traits![Prop, Shorthand, KeyValue, Assign, Getter, Setter, Method];
generate_traits![PropName, Ident, Str, Num, Computed];
generate_traits![Pat, Ident, Array, Rest, Object, Assign, Invalid, Expr];
generate_traits![TsType, TsKeywordType, TsThisType, TsFnOrConstructorType, TsTypeRef, TsTypeQuery, TsTypeLit, TsArrayType, TsTupleType,
    TsOptionalType, TsRestType, TsUnionOrIntersectionType, TsConditionalType, TsInferType, TsParenthesizedType, TsTypeOperator, TsIndexedAccessType,
    TsMappedType, TsLitType, TsTypePredicate, TsImportType];
generate_traits![TsFnOrConstructorType, TsFnType, TsConstructorType];
generate_traits![TsParamPropParam, Ident, Assign];
generate_traits![TsUnionOrIntersectionType, TsUnionType, TsIntersectionType];
generate_traits![DefaultDecl, Class, Fn, TsInterfaceDecl];
generate_traits![TsEntityName, TsQualifiedName, Ident];
generate_traits![ExprOrSuper, Super, Expr];
generate_traits![TsModuleName, Ident, Str];
generate_traits![TsNamespaceBody, TsModuleBlock, TsNamespaceDecl];
generate_traits![ModuleDecl, Import, ExportDecl, ExportNamed, ExportDefaultDecl, ExportDefaultExpr, ExportAll, TsImportEquals, TsExportAssignment,
    TsNamespaceExport];
generate_traits![TsModuleRef, TsEntityName, TsExternalModuleRef];
generate_traits![Stmt, Block, Empty, Debugger, With, Return, Labeled, Break, Continue, If, Switch, Throw, Try, While, DoWhile, For, ForIn, ForOf,
    Decl, Expr];

/* manual From implementations */

impl From<Box<Expr>> for Node {
    fn from(expr: Box<Expr>) -> Node {
        (*expr).into()
    }
}

impl From<Box<JSXElement>> for Node {
    fn from(element: Box<JSXElement>) -> Node {
        (*element).into()
    }
}

impl From<Box<Pat>> for Node {
    fn from(pat: Box<Pat>) -> Node {
        (*pat).into()
    }
}

impl From<Box<Prop>> for Node {
    fn from(prop: Box<Prop>) -> Node {
        (*prop).into()
    }
}

impl From<Box<TsQualifiedName>> for Node {
    fn from(qualified_name: Box<TsQualifiedName>) -> Node {
        (*qualified_name).into()
    }
}

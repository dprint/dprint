use std::str;
use std::rc::Rc;
use super::*;
use std::collections::{HashSet, HashMap};
use dprint_core::{Info, ConditionReference};
use utils::{Stack};
use swc_common::{SpanData, BytePos, comments::{Comment}, SourceFile, Spanned, Span};
use swc_ecma_ast::*;
use swc_ecma_parser::{token::{TokenAndSpan}};

pub struct Context<'a> {
    pub config: Configuration,
    pub comments: CommentCollection<'a>,
    pub token_finder: TokenFinder<'a>,
    pub file_bytes: &'a Vec<u8>,
    pub current_node: Node<'a>,
    pub parent_stack: Stack<Node<'a>>,
    handled_comments: HashSet<BytePos>,
    pub info: Rc<SourceFile>,
    stored_infos: HashMap<BytePos, Info>,
    pub end_statement_or_member_infos: Stack<Info>,
    disable_indent_for_next_bin_expr: bool,
    if_stmt_last_brace_condition_ref: Option<ConditionReference>,
}

impl<'a> Context<'a> {
    pub fn new(
        config: Configuration,
        leading_comments: &'a HashMap<BytePos, Vec<Comment>>,
        trailing_comments: &'a HashMap<BytePos, Vec<Comment>>,
        tokens: &'a Vec<TokenAndSpan>,
        file_bytes: &'a Vec<u8>,
        current_node: Node<'a>,
        info: SourceFile
    ) -> Context<'a> {
        Context {
            config,
            comments: CommentCollection::new(leading_comments, trailing_comments, tokens, file_bytes),
            token_finder: TokenFinder::new(tokens, file_bytes),
            file_bytes,
            current_node,
            parent_stack: Stack::new(),
            handled_comments: HashSet::new(),
            info: Rc::new(info),
            stored_infos: HashMap::new(),
            end_statement_or_member_infos: Stack::new(),
            disable_indent_for_next_bin_expr: false,
            if_stmt_last_brace_condition_ref: None,
        }
    }

    pub fn parent(&self) -> &Node<'a> {
        self.parent_stack.peek().unwrap()
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

    pub fn get_info_for_node(&self, node: &dyn Ranged) -> Option<Info> {
        self.stored_infos.get(&node.lo()).map(|x| x.to_owned())
    }

    pub fn mark_disable_indent_for_next_bin_expr(&mut self) {
        self.disable_indent_for_next_bin_expr = true;
    }

    pub fn get_disable_indent_for_next_bin_expr(&mut self) -> bool {
        let value = self.disable_indent_for_next_bin_expr;
        self.disable_indent_for_next_bin_expr = false;
        return value;
    }

    pub fn store_if_stmt_last_brace_condition_ref(&mut self, condition_reference: ConditionReference) {
        self.if_stmt_last_brace_condition_ref = Some(condition_reference);
    }

    pub fn take_if_stmt_last_brace_condition_ref(&mut self) -> Option<ConditionReference> {
        self.if_stmt_last_brace_condition_ref.take()
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
    fn start_column(&self, context: &mut Context) -> usize;
    fn text<'a>(&self, context: &'a Context) -> &'a str;
    fn leading_comments<'a>(&self, context: &mut Context<'a>) -> CommentsIterator<'a>;
    fn trailing_comments<'a>(&self, context: &mut Context<'a>) -> CommentsIterator<'a>;
}

impl<T> Ranged for T where T : Spanned {
    fn lo(&self) -> BytePos {
        self.span().lo()
    }

    fn hi(&self) -> BytePos {
        self.span().hi()
    }

    fn start_line(&self, context: &mut Context) -> usize {
        context.info.lookup_line(self.lo()).unwrap_or(0) + 1
    }

    fn end_line(&self, context: &mut Context) -> usize {
        context.info.lookup_line(self.hi()).unwrap_or(0) + 1
    }

    fn start_column(&self, context: &mut Context) -> usize {
        // not exactly correct because this isn't char based, but this is fast
        // and good enough for doing comparisons
        let pos = self.lo().0 as usize;
        for i in (0..pos).rev() {
            if context.file_bytes[i] == '\n' as u8 {
                return pos - i + 1;
            }
        }
        return pos;
    }

    fn text<'a>(&self, context: &'a Context) -> &'a str {
        let span_data = self.span().data();
        context.get_text(&span_data)
    }

    fn leading_comments<'a>(&self, context: &mut Context<'a>) -> CommentsIterator<'a> {
        context.comments.leading_comments(self.lo())
    }

    fn trailing_comments<'a>(&self, context: &mut Context<'a>) -> CommentsIterator<'a> {
        context.comments.trailing_comments(self.hi())
    }
}

macro_rules! generate_node {
    ($($node_name:ident),*) => {
        #[derive(Clone, PartialEq, Debug)]
        pub enum NodeKind {
            $($node_name),*,
        }

        #[derive(Clone)]
        pub enum Node<'a> {
            $($node_name(&'a $node_name)),*
        }

        impl<'a> NodeKinded for Node<'a> {
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
        impl<'a> From<&'a $node_name> for Node<'a> {
            fn from(node: &'a $node_name) -> Node<'a> {
                Node::$node_name(node)
            }
        }

        impl<'a> From<&'a Box<$node_name>> for Node<'a> {
            fn from(boxed_node: &'a Box<$node_name>) -> Node<'a> {
                (&**boxed_node).into()
            }
        }
        )*

        impl<'a> Spanned for Node<'a> {
            fn span(&self) -> Span {
                match self {
                    $(Node::$node_name(node) => node.span()),*
                }
            }
        }
    };
}

generate_node! [
    /* class */
    ClassMethod,
    ClassProp,
    Constructor,
    Decorator,
    PrivateMethod,
    PrivateProp,
    TsParamProp,
    /* clauses */
    CatchClause,
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
    TplElement,
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
    /* jsx */
    JSXAttr,
    JSXClosingElement,
    JSXClosingFragment,
    JSXElement,
    JSXEmptyExpr,
    JSXExprContainer,
    JSXFragment,
    JSXMemberExpr,
    JSXNamespacedName,
    JSXOpeningElement,
    JSXOpeningFragment,
    JSXSpreadChild,
    JSXText,
    /* literals */
    BigInt,
    Bool,
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
    SwitchCase,
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
    Span,
    TokenAndSpan
];

/* custom enums */

pub enum TypeParamNode<'a> {
    Instantiation(&'a TsTypeParamInstantiation),
    Decl(&'a TsTypeParamDecl)
}

impl<'a> TypeParamNode<'a> {
    pub fn params(&self) -> Vec<Node<'a>> {
        match self {
            TypeParamNode::Instantiation(node) => node.params.iter().map(|p| p.into()).collect(),
            TypeParamNode::Decl(node) => node.params.iter().map(|p| p.into()).collect(),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            TypeParamNode::Instantiation(node) => node.span,
            TypeParamNode::Decl(node) => node.span,
        }
    }
}

pub enum NamedImportOrExportDeclaration<'a> {
    Import(&'a ImportDecl),
    Export(&'a NamedExport),
}

impl<'a> From<NamedImportOrExportDeclaration<'a>> for Node<'a> {
    fn from(node: NamedImportOrExportDeclaration<'a>) -> Node<'a> {
        match node {
            NamedImportOrExportDeclaration::Import(node) => node.into(),
            NamedImportOrExportDeclaration::Export(node) => node.into(),
        }
    }
}

/* fully implemented From and NodeKinded implementations */

macro_rules! generate_traits {
    ($enum_name:ident, $($member_name:ident),*) => {
        impl<'a> From<&'a $enum_name> for Node<'a> {
            fn from(id: &'a $enum_name) -> Node<'a> {
                match id {
                    $($enum_name::$member_name(node) => node.into()),*
                }
            }
        }

        impl<'a> From<&'a Box<$enum_name>> for Node<'a> {
            fn from(boxed_node: &'a Box<$enum_name>) -> Node<'a> {
                (&**boxed_node).into()
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
generate_traits![ObjectPatProp, KeyValue, Assign, Rest];
generate_traits![PatOrExpr, Pat, Expr];
generate_traits![TsEnumMemberId, Ident, Str];
generate_traits![TsLit, Number, Str, Bool];
generate_traits![TsTypeElement, TsCallSignatureDecl, TsConstructSignatureDecl, TsPropertySignature, TsMethodSignature, TsIndexSignature];
generate_traits![TsFnParam, Ident, Array, Rest, Object];
generate_traits![Expr, This, Array, Object, Fn, Unary, Update, Bin, Assign, Member, Cond, Call, New, Seq, Ident, Lit, Tpl, TaggedTpl, Arrow,
    Class, Yield, MetaProp, Await, Paren, JSXMember, JSXNamespacedName, JSXEmpty, JSXElement, JSXFragment, TsTypeAssertion, TsConstAssertion,
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
generate_traits![TsThisTypeOrIdent, TsThisType, Ident];
generate_traits![TsTypeQueryExpr, TsEntityName, Import];
generate_traits![TsUnionOrIntersectionType, TsUnionType, TsIntersectionType];
generate_traits![DefaultDecl, Class, Fn, TsInterfaceDecl];
generate_traits![TsEntityName, TsQualifiedName, Ident];
generate_traits![ExprOrSuper, Super, Expr];
generate_traits![TsModuleName, Ident, Str];
generate_traits![VarDeclOrPat, VarDecl, Pat];
generate_traits![VarDeclOrExpr, VarDecl, Expr];
generate_traits![TsNamespaceBody, TsModuleBlock, TsNamespaceDecl];
generate_traits![PatOrTsParamProp, Pat, TsParamProp];
generate_traits![ModuleDecl, Import, ExportDecl, ExportNamed, ExportDefaultDecl, ExportDefaultExpr, ExportAll, TsImportEquals, TsExportAssignment,
    TsNamespaceExport];
generate_traits![TsModuleRef, TsEntityName, TsExternalModuleRef];
generate_traits![Stmt, Block, Empty, Debugger, With, Return, Labeled, Break, Continue, If, Switch, Throw, Try, While, DoWhile, For, ForIn, ForOf,
    Decl, Expr];
generate_traits![JSXElementChild, JSXText, JSXExprContainer, JSXSpreadChild, JSXElement, JSXFragment];
generate_traits![JSXAttrName, Ident, JSXNamespacedName];
generate_traits![JSXAttrOrSpread, JSXAttr, SpreadElement];
generate_traits![JSXElementName, Ident, JSXMemberExpr, JSXNamespacedName];
generate_traits![JSXAttrValue, Lit, JSXExprContainer, JSXElement, JSXFragment];
generate_traits![JSXExpr, JSXEmptyExpr, Expr];
generate_traits![JSXObject, JSXMemberExpr, Ident];

pub trait InnerSpanned {
    fn get_inner_span(&self, context: &mut Context) -> Span;
}

impl InnerSpanned for BlockStmt {
    fn get_inner_span(&self, _: &mut Context) -> Span {
        get_inner_span_for_object_like(&self.span)
    }
}

impl InnerSpanned for ObjectLit {
    fn get_inner_span(&self, _: &mut Context) -> Span {
        get_inner_span_for_object_like(&self.span)
    }
}

impl InnerSpanned for ObjectPat {
    fn get_inner_span(&self, _: &mut Context) -> Span {
        get_inner_span_for_object_like(&self.span)
    }
}

fn get_inner_span_for_object_like(span: &Span) -> Span {
    let span_data = span.data();
    return Span::new(
        BytePos(span_data.lo.0 + 1),
        BytePos(span_data.hi.0 - 1),
        Default::default()
    );
}

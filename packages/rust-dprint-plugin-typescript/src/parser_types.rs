use std::str;
use std::rc::Rc;
use super::*;
use std::collections::{HashSet, HashMap};
use dprint_core::{Info};
use utils::{Stack};
use swc_common::{SpanData, BytePos, comments::{Comment}, SourceFile, Spanned, Span};
use swc_ecma_ast::*;
use swc_ecma_parser::{token::{TokenAndSpan}};

pub struct Context {
    pub config: TypeScriptConfiguration,
    pub comments: CommentCollection,
    pub token_finder: TokenFinder,
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
        comments: CommentCollection,
        token_finder: TokenFinder,
        file_bytes: Rc<Vec<u8>>,
        current_node: Node,
        info: SourceFile
    ) -> Context {
        Context {
            config,
            comments,
            token_finder,
            file_bytes: file_bytes,
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
    fn leading_comments(&self, context: &mut Context) -> Vec<Comment>;
    fn trailing_comments(&self, context: &mut Context) -> Vec<Comment>;
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

    fn start_column(&self, context: &mut Context) -> usize {
        // not exactly correct because this isn't char based, but this is fast
        // and good enough for doing comparisons
        let pos = self.lo().0 as usize;
        for i in (0..pos).rev() {
            if context.file_bytes[i] == '\n' as u8 {
                return pos - i;
            }
        }
        return pos;
    }

    fn text<'a>(&self, context: &'a Context) -> &'a str {
        let span_data = self.span().data();
        context.get_text(&span_data)
    }

    fn leading_comments(&self, context: &mut Context) -> Vec<Comment> {
        context.comments.leading_comments(self.lo())
    }

    fn trailing_comments(&self, context: &mut Context) -> Vec<Comment> {
        context.comments.trailing_comments(self.hi())
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

        impl From<Box<$node_name>> for Node {
            fn from(boxed_node: Box<$node_name>) -> Node {
                (*boxed_node).into()
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
    JSXElement,
    JSXEmptyExpr,
    JSXFragment,
    JSXMemberExpr,
    JSXNamespacedName,
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

        impl From<Box<$enum_name>> for Node {
            fn from(boxed_node: Box<$enum_name>) -> Node {
                (*boxed_node).into()
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
generate_traits![ModuleDecl, Import, ExportDecl, ExportNamed, ExportDefaultDecl, ExportDefaultExpr, ExportAll, TsImportEquals, TsExportAssignment,
    TsNamespaceExport];
generate_traits![TsModuleRef, TsEntityName, TsExternalModuleRef];
generate_traits![Stmt, Block, Empty, Debugger, With, Return, Labeled, Break, Continue, If, Switch, Throw, Try, While, DoWhile, For, ForIn, ForOf,
    Decl, Expr];

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

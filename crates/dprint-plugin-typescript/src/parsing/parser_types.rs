use std::str;
use std::collections::{HashSet, HashMap};
use dprint_core::{Info, ConditionReference};
use swc_common::{SpanData, BytePos, comments::{Comment, CommentKind}, SourceFile, Spanned, Span};
use swc_ecma_ast::*;
use swc_ecma_parser::{token::TokenAndSpan};
use super::*;
use super::super::configuration::*;
use super::super::utils::Stack;

pub struct Context<'a> {
    pub config: &'a Configuration,
    pub comments: CommentCollection<'a>,
    pub token_finder: TokenFinder<'a>,
    pub file_bytes: &'a [u8],
    pub current_node: Node<'a>,
    pub parent_stack: Stack<Node<'a>>,
    handled_comments: HashSet<BytePos>,
    pub info: &'a SourceFile,
    stored_infos: HashMap<(BytePos, BytePos), Info>,
    stored_info_ranges: HashMap<(BytePos, BytePos), (Info, Info)>,
    pub end_statement_or_member_infos: Stack<Info>,
    if_stmt_last_brace_condition_ref: Option<ConditionReference>,
    /// Used for ensuring nodes are parsed in order.
    #[cfg(debug_assertions)]
    pub last_parsed_node_pos: u32,
}

impl<'a> Context<'a> {
    pub fn new(
        config: &'a Configuration,
        leading_comments: &'a HashMap<BytePos, Vec<Comment>>,
        trailing_comments: &'a HashMap<BytePos, Vec<Comment>>,
        tokens: &'a Vec<TokenAndSpan>,
        file_bytes: &'a [u8],
        current_node: Node<'a>,
        info: &'a SourceFile
    ) -> Context<'a> {
        Context {
            config,
            comments: CommentCollection::new(leading_comments, trailing_comments, tokens, file_bytes),
            token_finder: TokenFinder::new(tokens, file_bytes),
            file_bytes,
            current_node,
            parent_stack: Stack::new(),
            handled_comments: HashSet::new(),
            info,
            stored_infos: HashMap::new(),
            stored_info_ranges: HashMap::new(),
            end_statement_or_member_infos: Stack::new(),
            if_stmt_last_brace_condition_ref: None,
            #[cfg(debug_assertions)]
            last_parsed_node_pos: 0,
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

    pub fn get_text(&self, lo: BytePos, hi: BytePos) -> &str {
        let bytes = &self.file_bytes[(lo.0 as usize)..(hi.0 as usize)];
        str::from_utf8(&bytes).unwrap()
    }

    pub fn store_info_for_node(&mut self, node: &dyn Ranged, info: Info) {
        self.stored_infos.insert((node.lo(), node.hi()), info);
    }

    pub fn get_info_for_node(&self, node: &dyn Ranged) -> Option<Info> {
        self.stored_infos.get(&(node.lo(), node.hi())).map(|x| x.to_owned())
    }

    pub fn store_info_range_for_node(&mut self, node: &dyn Ranged, infos: (Info, Info)) {
        self.stored_info_ranges.insert((node.lo(), node.hi()), infos);
    }

    pub fn get_info_range_for_node(&self, node: &dyn Ranged) -> Option<(Info, Info)> {
        self.stored_info_ranges.get(&(node.lo(), node.hi())).map(|x| x.to_owned())
    }

    pub fn store_if_stmt_last_brace_condition_ref(&mut self, condition_reference: ConditionReference) {
        self.if_stmt_last_brace_condition_ref = Some(condition_reference);
    }

    pub fn take_if_stmt_last_brace_condition_ref(&mut self) -> Option<ConditionReference> {
        self.if_stmt_last_brace_condition_ref.take()
    }

    #[cfg(debug_assertions)]
    pub fn assert_text(&self, start_pos: BytePos, end_pos: BytePos, expected_text: &str) {
        let actual_text = self.get_text(start_pos, end_pos);
        if actual_text != expected_text {
            panic!("Expected text `{}`, but found `{}`", expected_text, actual_text)
        }
    }
}

pub trait NodeKinded {
    fn kind(&self) -> NodeKind;
}

pub trait SpanDataContainer {
    fn span_data(&self) -> SpanData;
}

impl SpanDataContainer for &dyn SpanDataContainer {
    fn span_data(&self) -> SpanData {
        (**self).span_data()
    }
}

impl SpanDataContainer for TokenAndSpan {
    fn span_data(&self) -> SpanData {
        self.span
    }
}

impl SpanDataContainer for Comment {
    fn span_data(&self) -> SpanData {
        self.span.data()
    }
}

impl SpanDataContainer for SpanData {
    fn span_data(&self) -> SpanData {
        self.clone()
    }
}

impl SpanDataContainer for BytePos {
    fn span_data(&self) -> SpanData {
        // todo: change this so this allocation isn't necessary
        SpanData {
            lo: *self,
            hi: *self,
            ctxt: Default::default(),
        }
    }
}

impl<T> SpanDataContainer for std::boxed::Box<T> where T : SpanDataContainer {
    fn span_data(&self) -> SpanData {
        (**self).span_data()
    }
}

pub trait Ranged : SpanDataContainer {
    fn lo(&self) -> BytePos;
    fn hi(&self) -> BytePos;
    fn start_line(&self, context: &mut Context) -> usize;
    /// Gets the start line with possible first leading comment start.
    fn start_line_with_comments(&self, context: &mut Context) -> usize;
    fn end_line(&self, context: &mut Context) -> usize;
    /// Gets the end line with possible trailing multi-line block comment end.
    fn end_line_with_comments(&self, context: &mut Context) -> usize;
    fn start_column(&self, context: &mut Context) -> usize;
    fn text<'a>(&self, context: &'a Context) -> &'a str;
    fn leading_comments<'a>(&self, context: &mut Context<'a>) -> CommentsIterator<'a>;
    fn trailing_comments<'a>(&self, context: &mut Context<'a>) -> CommentsIterator<'a>;
}

impl<T> Ranged for T where T : SpanDataContainer {
    fn lo(&self) -> BytePos {
        self.span_data().lo
    }

    fn hi(&self) -> BytePos {
        self.span_data().hi
    }

    fn start_line(&self, context: &mut Context) -> usize {
        context.info.lookup_line(self.lo()).unwrap_or(0) + 1
    }

    fn start_line_with_comments(&self, context: &mut Context) -> usize {
        // The start position with comments is the next non-whitespace position
        // after the previous token's trailing comments. The trailing comments
        // are similar to the Roslyn definition where it's any comments on the
        // same line or a single multi-line block comment that begins on the trailing line.
        let mut leading_comments = self.leading_comments(context);
        if leading_comments.is_empty() {
            self.start_line(context)
        } else {
            let lo = self.lo();
            let previous_token = context.token_finder.get_previous_token(&lo);
            if let Some(previous_token) = previous_token {
                let previous_end_line = previous_token.end_line(context);
                let mut past_trailing_comments = false;
                for comment in leading_comments {
                    let comment_start_line = comment.start_line(context);
                    if !past_trailing_comments && comment_start_line <= previous_end_line {
                        let comment_end_line = comment.end_line(context);
                        if comment_end_line > previous_end_line {
                            past_trailing_comments = true;
                        }
                    } else {
                        return comment_start_line;
                    }
                }

                self.start_line(context)
            } else {
                leading_comments.next().unwrap().start_line(context)
            }
        }
    }

    fn end_line(&self, context: &mut Context) -> usize {
        context.info.lookup_line(self.hi()).unwrap_or(0) + 1
    }

    fn end_line_with_comments(&self, context: &mut Context) -> usize {
        // start searching from after the trailing comma if it exists
        let search_end = context.token_finder.get_next_token_if_comma(self).map(|x| x.hi()).unwrap_or(self.hi());
        let trailing_comments = search_end.trailing_comments(context);
        let previous_end_line = search_end.end_line(context);
        for comment in trailing_comments {
            // optimization
            if comment.kind == CommentKind::Line { break; }

            let comment_start_line = comment.start_line(context);
            if comment_start_line <= previous_end_line {
                let comment_end_line = comment.end_line(context);
                if comment_end_line > previous_end_line {
                    return comment_end_line; // should only include the first multi-line comment block
                }
            } else {
                break;
            }
        }

        previous_end_line
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
        let span_data = self.span_data();
        context.get_text(span_data.lo, span_data.hi)
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
        impl SpanDataContainer for $node_name {
            fn span_data(&self) -> SpanData {
                self.span().data()
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

        impl<'a> SpanDataContainer for Node<'a> {
            fn span_data(&self) -> SpanData {
                match self {
                    $(Node::$node_name(node) => node.span().data()),*
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
    Param,
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
    Span
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

        impl SpanDataContainer for $enum_name {
            fn span_data(&self) -> SpanData {
                match self {
                    $($enum_name::$member_name(node) => node.span().data()),*
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
generate_traits![TsLit, Number, Str, Bool, Tpl];
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
generate_traits![ParamOrTsParamProp, Param, TsParamProp];
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

/* InnerSpanned */

pub trait InnerSpanned {
    fn get_inner_span_data(&self, context: &mut Context) -> SpanData;
}

impl InnerSpanned for BlockStmt {
    fn get_inner_span_data(&self, _: &mut Context) -> SpanData {
        get_inner_span_for_object_like(&self.span)
    }
}

impl InnerSpanned for ObjectLit {
    fn get_inner_span_data(&self, _: &mut Context) -> SpanData {
        get_inner_span_for_object_like(&self.span)
    }
}

impl InnerSpanned for ObjectPat {
    fn get_inner_span_data(&self, _: &mut Context) -> SpanData {
        get_inner_span_for_object_like(&self.span)
    }
}

fn get_inner_span_for_object_like(span: &Span) -> SpanData {
    let span_data = span.data();
    SpanData {
        lo: BytePos(span_data.lo.0 + 1),
        hi: BytePos(span_data.hi.0 - 1),
        ctxt: Default::default()
    }
}

/* ParametersSpanned */

pub trait ParametersSpanned {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData;
}

impl ParametersSpanned for Function {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.params.iter().map(|x| x.span_data()).collect(),
            self.return_type.as_ref().map(|t| t.lo())
                .or(self.body.as_ref().map(|b| b.lo()))
                .unwrap_or(self.span.hi()),
            context
        )
    }
}

impl ParametersSpanned for ClassMethod {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        self.function.get_parameters_span_data(context)
    }
}

impl ParametersSpanned for Constructor {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.params.iter().map(|x| x.span_data()).collect(),
            self.body.as_ref().map(|t| t.lo())
                .unwrap_or(self.span.hi()),
            context
        )
    }
}

impl ParametersSpanned for MethodProp {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        self.function.get_parameters_span_data(context)
    }
}

impl ParametersSpanned for GetterProp {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            vec![],
            self.type_ann.as_ref().map(|t| t.lo())
                .or(self.body.as_ref().map(|t| t.lo()))
                .unwrap_or(self.hi()),
            context
        )
    }
}

impl ParametersSpanned for SetterProp {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            vec![self.param.span_data()],
            self.body.as_ref().map(|t| t.lo()).unwrap_or(self.hi()),
            context
        )
    }
}

impl ParametersSpanned for ArrowExpr {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.params.iter().map(|x| x.span_data()).collect(),
            self.return_type.as_ref().map(|t| t.lo()).unwrap_or(self.body.lo()),
            context
        )
    }
}

impl ParametersSpanned for CallExpr {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.args.iter().map(|a| a.span_data()).collect(),
            self.hi(),
            context
        )
    }
}

impl ParametersSpanned for NewExpr {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.args.as_ref().map(|args| args.iter().map(|a| a.span_data()).collect()).unwrap_or_default(),
            self.hi(),
            context
        )
    }
}

impl ParametersSpanned for TsCallSignatureDecl {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.params.iter().map(|x| x.span_data()).collect(),
            self.type_ann.as_ref().map(|t| t.lo()).unwrap_or(self.hi()),
            context
        )
    }
}

impl ParametersSpanned for TsConstructSignatureDecl {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.params.iter().map(|x| x.span_data()).collect(),
            self.type_ann.as_ref().map(|t| t.lo()).unwrap_or(self.hi()),
            context
        )
    }
}

impl ParametersSpanned for TsMethodSignature {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.params.iter().map(|x| x.span_data()).collect(),
            self.type_ann.as_ref().map(|t| t.lo()).unwrap_or(self.hi()),
            context
        )
    }
}

impl ParametersSpanned for TsConstructorType {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.params.iter().map(|x| x.span_data()).collect(),
            self.type_ann.lo(),
            context
        )
    }
}

impl ParametersSpanned for TsFnType {
    fn get_parameters_span_data(&self, context: &mut Context) -> SpanData {
        get_params_or_args_span_data(
            self.params.iter().map(|x| x.span_data()).collect(),
            self.type_ann.lo(),
            context
        )
    }
}

fn get_params_or_args_span_data(params: Vec<SpanData>, following_pos: BytePos, context: &mut Context) -> SpanData {
    let close_token_end = {
        if let Some(last_param) = params.last() {
            context.token_finder.get_first_close_paren_after(last_param)
        } else {
            context.token_finder.get_first_close_paren_before(&following_pos)
        }.expect("Expected to find a close paren token.").hi()
    };

    SpanData {
        lo: context.token_finder.get_first_open_paren_before(&{
            if let Some(first_param) = params.first() {
                first_param.lo()
            } else {
                close_token_end
            }
        }).expect("Expected to find an close paren token.").lo(),
        hi: close_token_end,
        ctxt: Default::default(),
    }
}

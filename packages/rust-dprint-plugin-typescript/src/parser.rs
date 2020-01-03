extern crate dprint_core;

use dprint_core::*;
use dprint_core::{parser_helpers::*,condition_resolvers};
use super::*;
use super::configuration::{BracePosition, MemberSpacing, NextControlFlowPosition, OperatorPosition, SingleBodyPosition, TrailingCommas, UseBraces, UseParentheses};
use swc_ecma_ast::*;
use swc_common::{comments::{Comment, CommentKind}, Spanned, BytePos, Span, SpanData};
use swc_ecma_parser::{token::{Token, TokenAndSpan, Word, Keyword}};

pub fn parse(source_file: ParsedSourceFile, config: TypeScriptConfiguration) -> Vec<PrintItem> {
    let mut context = Context::new(
        config,
        source_file.comments,
        source_file.token_finder,
        source_file.file_bytes,
        Node::Module(source_file.module.clone()),
        source_file.info
    );
    let mut items = parse_node(Node::Module(source_file.module), &mut context);
    items.push(if_true(
        "endOfFileNewLine",
        |context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0),
        PrintItem::NewLine
    ));
    items
}

fn parse_node(node: Node, context: &mut Context) -> Vec<PrintItem> {
    parse_node_with_inner_parse(node, context, |items| items)
}

fn parse_node_with_inner_parse(node: Node, context: &mut Context, inner_parse: impl Fn(Vec<PrintItem>) -> Vec<PrintItem> + Clone + 'static) -> Vec<PrintItem> {
    let mut items = Vec::new();

    println!("Node kind: {:?}", node.kind());
    println!("Text: {:?}", node.text(context));

    // store info
    let past_current_node = std::mem::replace(&mut context.current_node, node.clone());
    let parent_span_data = past_current_node.span().data();
    context.parent_stack.push(past_current_node);

    // parse item

    // todo: need more robust comment scanning to ensure no comment is not handled (ex. getting comments before and after a token)

    let node_span = node.span();
    let node_span_data = node_span.data();
    items.extend(parse_leading_comments(&node, context));
    items.extend(inner_parse(parse_node_inner(node, context)));
    if context.parent().kind() == NodeKind::Module || node_span_data.hi != parent_span_data.hi {
        items.extend(parse_trailing_comments(&node_span, context));
    }

    // pop info
    context.current_node = context.parent_stack.pop().unwrap();

    return items;

    fn parse_node_inner(node: Node, context: &mut Context) -> Vec<PrintItem> {
        match node {
            /* class */
            Node::ClassMethod(node) => parse_class_method(node, context),
            Node::ClassProp(node) => parse_class_prop(node, context),
            Node::Decorator(node) => parse_decorator(node, context),
            Node::TsParamProp(node) => parse_parameter_prop(node, context),
            /* clauses */
            Node::CatchClause(node) => parse_catch_clause(node, context),
            /* common */
            Node::Ident(node) => parse_identifier(node, context),
            /* declarations */
            Node::ClassDecl(node) => parse_class_decl(node, context),
            Node::ExportDecl(node) => parse_export_decl(node, context),
            Node::ExportDefaultDecl(node) => parse_export_default_decl(node, context),
            Node::ExportDefaultExpr(node) => parse_export_default_expr(node, context),
            Node::FnDecl(node) => parse_function_decl(node, context),
            Node::ImportDecl(node) => parse_import_decl(node, context),
            Node::NamedExport(node) => parse_export_named_decl(node, context),
            Node::TsEnumDecl(node) => parse_enum_decl(node, context),
            Node::TsEnumMember(node) => parse_enum_member(node, context),
            Node::TsImportEqualsDecl(node) => parse_import_equals_decl(node, context),
            Node::TsInterfaceDecl(node) => parse_interface_decl(node, context),
            Node::TsModuleDecl(node) => parse_module_decl(node, context),
            Node::TsNamespaceDecl(node) => parse_namespace_decl(node, context),
            Node::TsTypeAliasDecl(node) => parse_type_alias(node, context),
            /* expressions */
            Node::ArrayLit(node) => parse_array_expr(node, context),
            Node::ArrowExpr(node) => parse_arrow_func_expr(node, context),
            Node::AssignExpr(node) => parse_assignment_expr(node, context),
            Node::AwaitExpr(node) => parse_await_expr(node, context),
            Node::BinExpr(node) => parse_binary_expr(node, context),
            Node::CallExpr(node) => parse_call_expr(node, context),
            Node::ClassExpr(node) => parse_class_expr(node, context),
            Node::CondExpr(node) => parse_conditional_expr(node, context),
            Node::ExprOrSpread(node) => parse_expr_or_spread(node, context),
            Node::FnExpr(node) => parse_fn_expr(node, context),
            Node::GetterProp(node) => parse_getter_prop(node, context),
            Node::KeyValueProp(node) => parse_key_value_prop(node, context),
            Node::MemberExpr(node) => parse_member_expr(node, context),
            Node::MetaPropExpr(node) => parse_meta_prop_expr(node, context),
            Node::NewExpr(node) => parse_new_expr(node, context),
            Node::ObjectLit(node) => parse_object_lit(node, context),
            Node::OptChainExpr(node) => parse_node(node.expr.into(), context),
            Node::ParenExpr(node) => parse_paren_expr(node, context),
            Node::SeqExpr(node) => parse_sequence_expr(node, context),
            Node::SetterProp(node) => parse_setter_prop(node, context),
            Node::SpreadElement(node) => parse_spread_element(node, context),
            Node::Super(_) => vec!["super".into()],
            Node::TaggedTpl(node) => parse_tagged_tpl(node, context),
            Node::TsAsExpr(node) => parse_as_expr(node, context),
            Node::TsExprWithTypeArgs(node) => parse_expr_with_type_args(node, context),
            Node::TsNonNullExpr(node) => parse_non_null_expr(node, context),
            Node::TsTypeAssertion(node) => parse_type_assertion(node, context),
            Node::UnaryExpr(node) => parse_unary_expr(node, context),
            Node::UpdateExpr(node) => parse_update_expr(node, context),
            Node::YieldExpr(node) => parse_yield_expr(node, context),
            /* exports */
            Node::NamedExportSpecifier(node) => parse_export_named_specifier(node, context),
            /* imports */
            Node::ImportSpecific(node) => parse_import_named_specifier(node, context),
            Node::ImportStarAs(node) => parse_import_namespace_specifier(node, context),
            Node::ImportDefault(node) => parse_node(node.local.into(), context),
            Node::TsExternalModuleRef(node) => parse_external_module_ref(node, context),
            /* interface / type element */
            Node::TsCallSignatureDecl(node) => parse_call_signature_decl(node, context),
            Node::TsConstructSignatureDecl(node) => parse_construct_signature_decl(node, context),
            Node::TsIndexSignature(node) => parse_index_signature(node, context),
            Node::TsInterfaceBody(node) => parse_interface_body(node, context),
            Node::TsMethodSignature(node) => parse_method_signature(node, context),
            Node::TsPropertySignature(node) => parse_property_signature(node, context),
            Node::TsTypeLit(node) => parse_type_lit(node, context),
            /* literals */
            Node::BigInt(node) => parse_big_int_literal(node, context),
            Node::Bool(node) => parse_bool_literal(node),
            Node::JSXText(node) => parse_jsx_text(node, context),
            Node::Null(_) => vec!["null".into()],
            Node::Number(node) => parse_num_literal(node, context),
            Node::Regex(node) => parse_reg_exp_literal(node, context),
            Node::Str(node) => parse_string_literal(node, context),
            /* module */
            Node::Module(node) => parse_module(node, context),
            /* patterns */
            Node::ArrayPat(node) => parse_array_pat(node, context),
            Node::AssignPat(node) => parse_assign_pat(node, context),
            Node::AssignPatProp(node) => parse_assign_pat_prop(node, context),
            Node::RestPat(node) => parse_rest_pat(node, context),
            Node::ObjectPat(node) => parse_object_pattern(node, context),
            /* properties */
            Node::MethodProp(node) => parse_method_prop(node, context),
            /* statements */
            Node::BlockStmt(node) => parse_block_stmt(node, context),
            Node::BreakStmt(node) => parse_break_stmt(node, context),
            Node::ContinueStmt(node) => parse_continue_stmt(node, context),
            Node::DebuggerStmt(node) => parse_debugger_stmt(node, context),
            Node::DoWhileStmt(node) => parse_do_while_stmt(node, context),
            Node::ExportAll(node) => parse_export_all(node, context),
            Node::ExprStmt(node) => parse_expr_stmt(node, context),
            Node::EmptyStmt(node) => parse_empty_stmt(node, context),
            Node::ForInStmt(node) => parse_for_in_stmt(node, context),
            Node::ForOfStmt(node) => parse_for_of_stmt(node, context),
            Node::ForStmt(node) => parse_for_stmt(node, context),
            Node::IfStmt(node) => parse_if_stmt(node, context),
            Node::LabeledStmt(node) => parse_labeled_stmt(node, context),
            Node::ReturnStmt(node) => parse_return_stmt(node, context),
            Node::SwitchStmt(node) => parse_switch_stmt(node, context),
            Node::SwitchCase(node) => parse_switch_case(node, context),
            Node::ThrowStmt(node) => parse_throw_stmt(node, context),
            Node::TryStmt(node) => parse_try_stmt(node, context),
            Node::TsExportAssignment(node) => parse_export_assignment(node, context),
            Node::TsNamespaceExportDecl(node) => parse_namespace_export(node, context),
            Node::VarDecl(node) => parse_var_decl(node, context),
            Node::VarDeclarator(node) => parse_var_declarator(node, context),
            Node::WhileStmt(node) => parse_while_stmt(node, context),
            /* types */
            Node::TsArrayType(node) => parse_array_type(node, context),
            Node::TsImportType(node) => parse_import_type(node, context),
            Node::TsLitType(node) => parse_lit_type(node, context),
            Node::TsTypeAnn(node) => parse_type_ann(node, context),
            Node::TsTypeParamInstantiation(node) => parse_type_param_instantiation(TypeParamNode::Instantiation(node), context),
            Node::TsTypeParamDecl(node) => parse_type_param_instantiation(TypeParamNode::Decl(node), context),
            /* unknown */
            Node::TokenAndSpan(span) => vec![context.get_text(&span.span.data()).into()],
            Node::Comment(comment) => vec![context.get_text(&comment.span.data()).into()],
            Node::Unknown(span) => vec![context.get_text(&span.data()).into()],
            _ => vec![node.text(context).into()]
        }
    }
}

/* class */

fn parse_class_method(node: ClassMethod, context: &mut Context) -> Vec<PrintItem> {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: node.function.decorators,
        accessibility: node.accessibility,
        is_static: node.is_static,
        is_async: node.function.is_async,
        is_abstract: node.is_abstract,
        kind: node.kind.into(),
        is_generator: node.function.is_generator,
        is_optional: node.is_optional,
        key: node.key.into(),
        type_params: node.function.type_params.map(|x| x.into()),
        params: node.function.params.into_iter().map(|x| x.into()).collect(),
        return_type: node.function.return_type.map(|x| x.into()),
        body: node.function.body.map(|x| x.into()),
    }, context);
}

fn parse_class_prop(node: ClassProp, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.extend(parse_decorators(node.decorators, false, context));
    if let Some(accessibility) = node.accessibility {
        items.push(format!("{} ", accessibility_to_str(&accessibility)).into());
    }
    if node.is_static { items.push("static ".into()); }
    if node.is_abstract { items.push("abstract ".into()); }
    if node.readonly { items.push("readonly ".into()); }
    if node.computed { items.push("[".into()); }
    items.extend(parse_node(node.key.into(), context));
    if node.computed { items.push("]".into()); }
    if node.is_optional { items.push("?".into()); }
    if node.definite { items.push("!".into()); }
    items.extend(parse_type_annotation_with_colon_if_exists(node.type_ann, context));

    if let Some(box value) = node.value {
        items.push(" = ".into());
        items.extend(parse_node(value.into(), context));
    }

    if context.config.class_property_semi_colon {
        items.push(";".into());
    }

    return items;
}

fn parse_decorator(node: Decorator, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("@".into());
    items.extend(parse_node(node.expr.into(), context));
    return items;
}

fn parse_parameter_prop(node: TsParamProp, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if let Some(accessibility) = node.accessibility {
        items.push(format!("{} ", accessibility_to_str(&accessibility)).into());
    }
    if node.readonly { items.push("readonly ".into()); }
    items.extend(parse_node(node.param.into(), context));
    return items;
}

/* clauses */

fn parse_catch_clause(node: CatchClause, context: &mut Context) -> Vec<PrintItem> {
    // a bit overkill since the param will currently always just be an identifer
    let start_header_info = Info::new("catchClauseHeaderStart");
    let end_header_info = Info::new("catchClauseHeaderEnd");
    let mut items = Vec::new();

    items.push(start_header_info.clone().into());
    items.push("catch".into());

    if let Some(param) = node.param {
        items.push(" (".into());
        items.extend(parse_node(param.into(), context));
        items.push(")".into());
    }
    items.push(end_header_info.clone().into());

    // not conditional... required
    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: node.body.into(),
        use_braces: UseBraces::Always,
        brace_position: context.config.try_statement_brace_position,
        single_body_position: None,
        requires_braces_condition: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);

    return items;
}

/* common */

fn parse_identifier(node: Ident, context: &mut Context) -> Vec<PrintItem> {
    let mut items: Vec<PrintItem> = Vec::new();
    items.push((&node.sym as &str).into());

    if node.optional {
        items.push("?".into());
    }
    if let Node::VarDeclarator(node) = context.parent() {
        if node.definite {
            items.push("!".into());
        }
    }

    items.extend(parse_type_annotation_with_colon_if_exists(node.type_ann, context));

    items
}

/* declarations */

fn parse_class_decl(node: ClassDecl, context: &mut Context) -> Vec<PrintItem> {
    return parse_class_decl_or_expr(ClassDeclOrExpr {
        span: node.class.span,
        decorators: node.class.decorators,
        is_class_expr: false,
        is_declare: node.declare,
        is_abstract: node.class.is_abstract,
        ident: Some(node.ident.into()),
        type_params: node.class.type_params.map(|x| x.into()),
        super_class: node.class.super_class.map(|x| x.into()),
        super_type_params: node.class.super_type_params.map(|x| x.into()),
        implements: node.class.implements.into_iter().map(|x| x.into()).collect(),
        members: node.class.body.into_iter().map(|x| x.into()).collect(),
        brace_position: context.config.class_declaration_brace_position,
    }, context);
}

struct ClassDeclOrExpr {
    span: Span,
    decorators: Vec<Decorator>,
    is_class_expr: bool,
    is_declare: bool,
    is_abstract: bool,
    ident: Option<Node>,
    type_params: Option<Node>,
    super_class: Option<Node>,
    super_type_params: Option<Node>,
    implements: Vec<Node>,
    members: Vec<Node>,
    brace_position: BracePosition,
}

fn parse_class_decl_or_expr(node: ClassDeclOrExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    let parent_kind = context.parent().kind();
    if parent_kind != NodeKind::ExportDecl && parent_kind != NodeKind::ExportDefaultDecl {
        items.extend(parse_decorators(node.decorators, node.is_class_expr, context));
    }

    let start_header_info = Info::new("startHeader");
    items.push(start_header_info.clone().into());

    if node.is_declare { items.push("declare ".into()); }
    if node.is_abstract { items.push("abstract ".into()); }

    items.push("class".into());

    if let Some(ident) = node.ident {
        items.push(" ".into());
        items.extend(parse_node(ident, context));
    }
    if let Some(type_params) = node.type_params {
        items.extend(parse_node(type_params, context));
    }
    if let Some(super_class) = node.super_class {
        items.push(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_header_info.clone(), None).into());
        items.push(conditions::indent_if_start_of_line({
            let mut items = Vec::new();
            items.push("extends ".into());
            items.extend(parse_node(super_class, context));
            if let Some(super_type_params) = node.super_type_params {
                items.extend(parse_node(super_type_params, context));
            }
            items
        }).into());
    }
    items.extend(parse_extends_or_implements("implements", node.implements, start_header_info.clone(), context));

    // parse body
    items.extend(parse_membered_body(ParseMemberedBodyOptions {
        span: node.span,
        members: node.members,
        start_header_info: Some(start_header_info),
        brace_position: node.brace_position,
        should_use_blank_line: Box::new(move |previous, next, context| {
            node_helpers::has_separating_blank_line(previous, next, context)
        }),
        trailing_commas: None,
    }, context));

    return items;
}

fn parse_export_decl(node: ExportDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if let Decl::Class(class_decl) = &node.decl {
        items.extend(parse_decorators(class_decl.class.decorators.clone(), false, context));
    }
    items.push("export ".into());
    items.extend(parse_node(node.decl.into(), context));
    items
}

fn parse_export_default_decl(node: ExportDefaultDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if let DefaultDecl::Class(class_expr) = &node.decl {
        items.extend(parse_decorators(class_expr.class.decorators.clone(), false, context));
    }
    items.push("export default ".into());
    items.extend(parse_node(node.decl.into(), context));
    items
}

fn parse_export_default_expr(node: ExportDefaultExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("export default ".into());
    items.extend(parse_node((*node.expr).into(), context));
    if context.config.export_default_expression_semi_colon { items.push(";".into()); }
    items
}

fn parse_enum_decl(node: TsEnumDecl, context: &mut Context) -> Vec<PrintItem> {
    let start_header_info = Info::new("startHeader");
    let mut items = Vec::new();

    // header
    items.push(start_header_info.clone().into());

    if node.declare { items.push("declare ".into()); }
    if node.is_const { items.push("const ".into()); }
    items.push("enum ".into());
    items.extend(parse_node(node.id.into(), context));

    // body
    let member_spacing = context.config.enum_declaration_member_spacing;
    items.extend(parse_membered_body(ParseMemberedBodyOptions {
        span: node.span,
        members: node.members.into_iter().map(|x| x.into()).collect(),
        start_header_info: Some(start_header_info),
        brace_position: context.config.enum_declaration_brace_position,
        should_use_blank_line: Box::new(move |previous, next, context| {
            match member_spacing {
                MemberSpacing::BlankLine => true,
                MemberSpacing::NewLine => false,
                MemberSpacing::Maintain => node_helpers::has_separating_blank_line(previous, next, context),
            }
        }),
        trailing_commas: Some(context.config.enum_declaration_trailing_commas),
    }, context));

    return items;
}

fn parse_enum_member(node: TsEnumMember, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_node(node.id.into(), context);

    if let Some(box init) = node.init {
        items.push(match init.kind() {
            NodeKind::Number | NodeKind::Str => PrintItem::SpaceOrNewLine,
            _ => " ".into(),
        });

        items.push(conditions::indent_if_start_of_line({
            let mut items = Vec::new();
            items.push("= ".into());
            items.extend(parse_node(init.into(), context));
            items
        }).into());
    }

    items
}

fn parse_export_named_decl(node: NamedExport, context: &mut Context) -> Vec<PrintItem> {
    // todo: rewrite this so that it doesn't need to clone the current node

    // fill specifiers
    let mut default_export: Option<DefaultExportSpecifier> = None;
    let mut namespace_export: Option<NamespaceExportSpecifier> = None;
    let mut named_exports: Vec<NamedExportSpecifier> = Vec::new();
    let decl = NamedImportOrExportDeclaration::Export(node.clone()); // todo: rewrite without this

    for specifier in node.specifiers {
        match specifier {
            ExportSpecifier::Default(node) => default_export = Some(node),
            ExportSpecifier::Namespace(node) => namespace_export = Some(node),
            ExportSpecifier::Named(node) => named_exports.push(node),
        }
    }

    // parse
    let mut items = Vec::new();
    let node_src = node.src;

    items.push("export ".into());

    if let Some(default_export) = default_export {
        items.extend(parse_node(default_export.into(), context));
    } else if !named_exports.is_empty() {
        items.extend(parse_named_import_or_export_specifiers(
            decl,
            named_exports.into_iter().map(|x| x.into()).collect(),
            context
        ));
    } else if let Some(namespace_export) = namespace_export {
        items.extend(parse_node(namespace_export.into(), context));
    } else {
        items.push("{}".into());
    }

    if let Some(src) = node_src {
        items.push(" from ".into());
        items.extend(parse_node(src.into(), context));
    }

    if context.config.export_named_declaration_semi_colon {
        items.push(";".into());
    }

    items
}

fn parse_function_decl(node: FnDecl, context: &mut Context) -> Vec<PrintItem> {
    parse_function_decl_or_expr(FunctionDeclOrExprNode {
        is_func_decl: true,
        ident: Some(node.ident),
        declare: node.declare,
        func: node.function,
    }, context)
}

struct FunctionDeclOrExprNode {
    is_func_decl: bool,
    ident: Option<Ident>,
    declare: bool,
    func: Function,
}

fn parse_function_decl_or_expr(node: FunctionDeclOrExprNode, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let start_header_info = Info::new("functionHeaderStart");
    let func = node.func;

    items.push(start_header_info.clone().into());
    if node.declare { items.push("declare ".into()); }
    if func.is_async { items.push("async ".into()); }
    items.push("function".into());
    if func.is_generator { items.push("*".into()); }
    if let Some(ident) = node.ident {
        items.push(" ".into());
        items.extend(parse_node(ident.into(), context));
    }
    if let Some(type_params) = func.type_params { items.extend(parse_node(type_params.into(), context)); }
    if get_use_space_before_parens(node.is_func_decl, context) { items.push(" ".into()); }

    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: func.params.into_iter().map(|node| node.into()).collect(),
        force_multi_line_when_multiple_lines: if node.is_func_decl {
            context.config.function_declaration_force_multi_line_parameters
        } else {
            context.config.function_expression_force_multi_line_parameters
        },
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info: start_header_info.clone(),
            type_node: func.return_type.map(|x| x.into()),
            type_node_separator: None,
        }, context)),
    }, context));

    if let Some(body) = func.body {
        let brace_position = if node.is_func_decl {
            context.config.function_declaration_brace_position
        } else {
            context.config.function_expression_brace_position
        };
        let open_brace_token = context.get_first_open_brace_token_within(&body);

        items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
            brace_position: brace_position,
            open_brace_token: &open_brace_token,
            start_header_info: Some(start_header_info),
        }, context));

        items.extend(parse_node(body.into(), context));
    } else {
        if context.config.function_declaration_semi_colon {
            items.push(";".into());
        }
    }

    return items;

    fn get_use_space_before_parens(is_func_decl: bool, context: &mut Context) -> bool {
        if is_func_decl {
            context.config.function_declaration_space_before_parentheses
        } else {
            context.config.function_expression_space_before_parentheses
        }
    }
}

fn parse_import_decl(node: ImportDecl, context: &mut Context) -> Vec<PrintItem> {
    // todo: rewrite this so that it doesn't need to clone the current node

    // fill specifiers
    let mut default_import: Option<ImportDefault> = None;
    let mut namespace_import: Option<ImportStarAs> = None;
    let mut named_imports: Vec<ImportSpecific> = Vec::new();
    let decl = NamedImportOrExportDeclaration::Import(node.clone()); // todo: rewrite without this

    for specifier in node.specifiers {
        match specifier {
            ImportSpecifier::Default(node) => default_import = Some(node),
            ImportSpecifier::Namespace(node) => namespace_import = Some(node),
            ImportSpecifier::Specific(node) => named_imports.push(node),
        }
    }

    let mut items = Vec::new();
    let has_from = default_import.is_some() || namespace_import.is_some() || !named_imports.is_empty();
    items.push("import ".into());

    if let Some(default_import) = default_import {
        items.extend(parse_node(default_import.into(), context));
        if namespace_import.is_some() || !named_imports.is_empty() {
            items.push(", ".into());
        }
    }
    if let Some(namespace_import) = namespace_import {
        items.extend(parse_node(namespace_import.into(), context));
    }
    items.extend(parse_named_import_or_export_specifiers(
        decl,
        named_imports.into_iter().map(|x| x.into()).collect(),
        context
    ));

    if has_from { items.push(" from ".into()); }

    items.extend(parse_node(node.src.into(), context));

    if context.config.import_declaration_semi_colon {
        items.push(";".into());
    }

    return items;
}

fn parse_import_equals_decl(node: TsImportEqualsDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if node.is_export {
        items.push("export ".into());
    }

    items.push("import ".into());
    items.extend(parse_node(node.id.into(), context));
    items.push(" = ".into());
    items.extend(parse_node(node.module_ref.into(), context));

    if context.config.import_equals_semi_colon { items.push(";".into()); }

    return items;
}

fn parse_interface_decl(node: TsInterfaceDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let start_header_info = Info::new("startHeader");
    items.push(start_header_info.clone().into());
    context.store_info_for_node(&node, start_header_info.clone());

    if node.declare { items.push("declare ".into()); }
    items.push("interface ".into());
    items.extend(parse_node(node.id.into(), context));
    if let Some(type_params) = node.type_params { items.extend(parse_node(type_params.into(), context)); }
    items.extend(parse_extends_or_implements("extends", node.extends.into_iter().map(|x| x.into()).collect(), start_header_info, context));
    items.extend(parse_node(node.body.into(), context));

    return items;
}

fn parse_module_decl(node: TsModuleDecl, context: &mut Context) -> Vec<PrintItem> {
    parse_module_or_namespace_decl(ModuleOrNamespaceDecl {
        span: node.span,
        declare: node.declare,
        global: node.global,
        id: node.id.into(),
        body: node.body
    }, context)
}

fn parse_namespace_decl(node: TsNamespaceDecl, context: &mut Context) -> Vec<PrintItem> {
    parse_module_or_namespace_decl(ModuleOrNamespaceDecl {
        span: node.span,
        declare: node.declare,
        global: node.global,
        id: node.id.into(),
        body: Some(*node.body)
    }, context)
}

struct ModuleOrNamespaceDecl {
    pub span: Span,
    pub declare: bool,
    pub global: bool,
    pub id: Node,
    pub body: Option<TsNamespaceBody>,
}

fn parse_module_or_namespace_decl(node: ModuleOrNamespaceDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    let start_header_info = Info::new("startHeader");
    items.push(start_header_info.clone().into());

    if node.declare { items.push("declare ".into()); }
    if node.global {
        items.push("global".into());
        // items.extend(parse_node(node.id.into(), context));
    } else {
        let has_namespace_keyword = context.get_token_text_at_pos(node.span.lo()) == Some("namespace");
        items.push(if has_namespace_keyword { "namespace " } else { "module " }.into());
    }

    items.extend(parse_node(node.id.into(), context));
    items.extend(parse_body(node.body, start_header_info, context));

    return items;

    fn parse_body(body: Option<TsNamespaceBody>, start_header_info: Info, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();
        if let Some(body) = body {
            match body {
                TsNamespaceBody::TsModuleBlock(block) => {
                    items.extend(parse_membered_body(ParseMemberedBodyOptions {
                        span: block.span,
                        members: block.body.into_iter().map(|x| x.into()).collect(),
                        start_header_info: Some(start_header_info),
                        brace_position: context.config.module_declaration_brace_position,
                        should_use_blank_line: Box::new(move |previous, next, context| {
                            node_helpers::has_separating_blank_line(previous, next, context)
                        }),
                        trailing_commas: None,
                    }, context));
                },
                TsNamespaceBody::TsNamespaceDecl(decl) => {
                    items.push(".".into());
                    items.extend(parse_node(decl.id.into(), context));
                    items.extend(parse_body(Some(*decl.body), start_header_info, context));
                }
            }
        }
        else if context.config.module_declaration_semi_colon {
            items.push(";".into());
        }

        return items;
    }
}

fn parse_type_alias(node: TsTypeAliasDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if node.declare { items.push("declare ".into()); }
    items.push("type ".into());
    items.extend(parse_node(node.id.into(), context));
    if let Some(type_params) = node.type_params {
        items.extend(parse_node(type_params.into(), context));
    }
    items.push(" = ".into());
    items.extend(parse_node((*node.type_ann).into(), context));

    if context.config.type_alias_semi_colon { items.push(";".into()); }

    return items;
}

/* exports */

fn parse_named_import_or_export_specifiers(parent_decl: NamedImportOrExportDeclaration, specifiers: Vec<Node>, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if specifiers.is_empty() {
        return items;
    }

    let use_space = get_use_space(&parent_decl, context);
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(
        &context.get_first_open_brace_token_within(&Node::from(parent_decl)),
        &specifiers[0],
        context
    );
    let brace_separator = if use_new_lines { PrintItem::NewLine } else { if use_space { " ".into() } else { "".into() } };

    items.push("{".into());
    items.push(brace_separator.clone());

    let specifiers = {
        let mut items = Vec::new();
        for (i, specifier) in specifiers.into_iter().enumerate() {
            if i > 0 {
                items.push(",".into());
                items.push(if use_new_lines { PrintItem::NewLine } else { PrintItem::SpaceOrNewLine });
            }

            let parsed_specifier = parse_node(specifier.into(), context);
            items.extend(if use_new_lines {
                parsed_specifier
            } else {
                vec![conditions::indent_if_start_of_line(parser_helpers::new_line_group(parsed_specifier)).into()]
            });
        }
        items
    };

    items.extend(if use_new_lines {
        parser_helpers::with_indent(specifiers)
    } else {
        specifiers
    });

    items.push(brace_separator);
    items.push("}".into());

    return items;

    fn get_use_space(parent_decl: &NamedImportOrExportDeclaration, context: &mut Context) -> bool {
        match parent_decl {
            NamedImportOrExportDeclaration::Export(_) => context.config.export_declaration_space_surrounding_named_exports,
            NamedImportOrExportDeclaration::Import(_) => context.config.import_declaration_space_surrounding_named_imports,
        }
    }
}

/* expressions */

fn parse_array_expr(node: ArrayLit, context: &mut Context) -> Vec<PrintItem> {
    parse_array_like_nodes(ParseArrayLikeNodesOptions {
        node: node.clone().into(),
        elements: node.elems.into_iter().map(|x| x.map(|elem| elem.into())).collect(),
        trailing_commas: context.config.array_expression_trailing_commas,
    }, context)
}

fn parse_arrow_func_expr(node: ArrowExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let header_start_info = Info::new("arrowFunctionExpressionHeaderStart");
    let should_use_params = get_should_use_params(&node, context);

    items.push(header_start_info.clone().into());
    if node.is_async { items.push("async ".into()); }
    if let Some(type_params) = node.type_params { items.extend(parse_node(type_params.into(), context)); }

    if should_use_params {
        items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
            nodes: node.params.into_iter().map(|node| node.into()).collect(),
            force_multi_line_when_multiple_lines: context.config.arrow_function_expression_force_multi_line_parameters,
            custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
                start_info: header_start_info.clone(),
                type_node: node.return_type.map(|x| x.into()),
                type_node_separator: None,
            }, context)),
        }, context));
    } else {
        items.extend(parse_node(node.params.into_iter().next().unwrap().into(), context));
    }

    items.push(" =>".into());

    let open_brace_token = match &node.body {
        BlockStmtOrExpr::BlockStmt(stmt) => context.get_first_open_brace_token_within(&stmt),
        _ => None,
    };
    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: context.config.arrow_function_expression_brace_position,
        open_brace_token: &open_brace_token,
        start_header_info: Some(header_start_info),
    }, context));

    items.extend(parse_node(node.body.into(), context));

    return items;

    fn get_should_use_params(node: &ArrowExpr, context: &mut Context) -> bool {
        let requires_parens = node.params.len() != 1 || node.return_type.is_some() || is_first_param_not_identifier_or_has_type_annotation(&node.params);

        return if requires_parens {
            true
        } else {
            match context.config.arrow_function_expression_use_parentheses {
                UseParentheses::Force => true,
                UseParentheses::PreferNone => false,
                UseParentheses::Maintain => has_parentheses(&node, context),
            }
        };

        fn is_first_param_not_identifier_or_has_type_annotation(params: &Vec<Pat>) -> bool {
            let first_param = params.iter().next();
            match first_param {
                Some(Pat::Ident(node)) => node.type_ann.is_some(),
                _ => true
            }
        }

        fn has_parentheses(node: &ArrowExpr, context: &mut Context) -> bool {
            if node.params.len() != 1 {
                true
            } else {
                context.get_token_at(node).token == Token::LParen
            }
        }
    }
}

fn parse_as_expr(node: TsAsExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_node((*node.expr).into(), context);
    items.push(" as ".into());
    items.push(conditions::with_indent_if_start_of_line_indented(parse_node((*node.type_ann).into(), context)).into());
    items
}

fn parse_assignment_expr(node: AssignExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_node(node.left.into(), context);
    items.push(format!(" {} ", node.op).into());
    items.push(conditions::with_indent_if_start_of_line_indented(parse_node((*node.right).into(), context)).into());
    items
}

fn parse_await_expr(node: AwaitExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("await ".into());
    items.extend(parse_node((*node.arg).into(), context));
    items
}

fn parse_binary_expr(node: BinExpr, context: &mut Context) -> Vec<PrintItem> {
    return if is_expression_breakable(&node.op) {
        inner_parse(node, context)
    } else {
        new_line_group(inner_parse(node, context))
    };

    fn inner_parse(node: BinExpr, context: &mut Context) -> Vec<PrintItem> {
        let operator_token = context.get_first_token_after_with_text(&node.left, node.op.as_str()).unwrap();
        let operator_position = get_operator_position(&node, &operator_token, context);
        let top_most_expr_start = get_top_most_binary_expr_pos(&node, context);
        let node_start = node.lo();
        let node_left = *node.left;
        let node_right = *node.right;
        let node_op = node.op;
        let use_space_surrounding_operator = get_use_space_surrounding_operator(&node_op, context);
        let is_top_most = top_most_expr_start == node_start;
        let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&node_left, &node_right, context);
        let top_most_info = get_or_set_top_most_info(top_most_expr_start, is_top_most, context);
        let mut items = Vec::new();

        if is_top_most {
            items.push(top_most_info.clone().into());
        }

        items.push(indent_if_necessary(node_left.lo(), top_most_expr_start, top_most_info.clone(), {
            let operator_position = operator_position.clone();
            let node_op = node_op.clone();
            let node_left_node = Node::from(node_left.clone());
            new_line_group_if_necessary(&node_left, parse_node_with_inner_parse(node_left_node, context, move |mut items| {
                if operator_position == OperatorPosition::SameLine {
                    if use_space_surrounding_operator {
                        items.push(" ".into());
                    }
                    items.push(node_op.as_str().into());
                }
                items
            }))
        }));

        items.extend(parse_comments_as_trailing(&operator_token, operator_token.trailing_comments(context), context));

        items.push(if use_new_lines {
            PrintItem::NewLine
        } else if use_space_surrounding_operator {
            PrintItem::SpaceOrNewLine
        } else {
            PrintItem::PossibleNewLine
        });

        items.push(indent_if_necessary(node_right.lo(), top_most_expr_start, top_most_info, {
            let mut items = Vec::new();
            items.extend(parse_comments_as_leading(&node_right, operator_token.leading_comments(context), context));
            items.extend(parse_node_with_inner_parse(node_right.clone().into(), context, move |items| {
                let mut new_items = Vec::new();
                if operator_position == OperatorPosition::NextLine {
                    new_items.push(node_op.as_str().into());
                    if use_space_surrounding_operator {
                        new_items.push(" ".into());
                    }
                }
                new_items.extend(new_line_group_if_necessary(&node_right, items));
                new_items
            }));
            items
        }));

        return items;
    }

    fn indent_if_necessary(current_node_start: BytePos, top_most_expr_start: BytePos, top_most_info: Info, items: Vec<PrintItem>) -> PrintItem {
        Condition::new("indentIfNecessaryForBinaryExpressions", ConditionProperties {
            condition: Box::new(move |condition_context| {
                // do not indent if this is the left-most node
                if top_most_expr_start == current_node_start {
                    return Some(false);
                }
                if let Some(top_most_info) = condition_context.get_resolved_info(&top_most_info) {
                    let is_same_indent = top_most_info.indent_level == condition_context.writer_info.indent_level;
                    return Some(is_same_indent && condition_resolvers::is_start_of_new_line(condition_context));
                }
                return None;
            }),
            true_path: Some(parser_helpers::with_indent(items.clone())),
            false_path: Some(items)
        }).into()
    }

    fn new_line_group_if_necessary(expr: &Expr, items: Vec<PrintItem>) -> Vec<PrintItem> {
        match expr {
            Expr::Bin(_) => items,
            _ => parser_helpers::new_line_group(items),
        }
    }

    fn get_or_set_top_most_info(top_most_expr_start: BytePos, is_top_most: bool, context: &mut Context) -> Info {
        if is_top_most {
            let info = Info::new("topBinaryOrLogicalExpressionStart");
            context.store_info_for_node(&top_most_expr_start, info);
        }
        return context.get_info_for_node(&top_most_expr_start).expect("Expected to have the top most expr info stored").clone();
    }

    fn get_top_most_binary_expr_pos(node: &BinExpr, context: &mut Context) -> BytePos {
        let mut top_most: Option<&BinExpr> = None;
        for ancestor in context.parent_stack.iter().rev() {
            if let Node::BinExpr(ancestor) = ancestor {
                top_most = Some(ancestor);
            } else {
                break;
            }
        }

        top_most.unwrap_or(node).lo()
    }

    fn is_expression_breakable(op: &BinaryOp) -> bool {
        match op {
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul
                | BinaryOp::Div => true,
            _ => false,
        }
    }

    fn get_use_space_surrounding_operator(op: &BinaryOp, context: &mut Context) -> bool {
        match op {
            BinaryOp::EqEq | BinaryOp::NotEq | BinaryOp::EqEqEq | BinaryOp::NotEqEq | BinaryOp::Lt | BinaryOp::LtEq
                | BinaryOp::Gt | BinaryOp::GtEq | BinaryOp::LogicalOr | BinaryOp::LogicalAnd | BinaryOp::In
                | BinaryOp::InstanceOf | BinaryOp::Exp | BinaryOp::NullishCoalescing => true,
            BinaryOp::LShift | BinaryOp::RShift | BinaryOp::ZeroFillRShift | BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul
                | BinaryOp::Div | BinaryOp::Mod | BinaryOp::BitOr | BinaryOp::BitXor
                | BinaryOp::BitAnd => context.config.binary_expression_space_surrounding_bitwise_and_arithmetic_operator,
        }
    }

    fn get_operator_position(node: &BinExpr, operator_token: &TokenAndSpan, context: &mut Context) -> OperatorPosition {
        match context.config.binary_expression_operator_position {
            OperatorPosition::NextLine => OperatorPosition::NextLine,
            OperatorPosition::SameLine => OperatorPosition::SameLine,
            OperatorPosition::Maintain => {
                if node.left.end_line(context) == operator_token.start_line(context) {
                    OperatorPosition::SameLine
                } else {
                    OperatorPosition::NextLine
                }
            }
        }
    }
}

fn parse_call_expr(node: CallExpr, context: &mut Context) -> Vec<PrintItem> {
    return if is_test_library_call_expr(&node, context) {
        parse_test_library_call_expr(node, context)
    } else {
        inner_parse(node, context)
    };

    fn inner_parse(node: CallExpr, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();

        items.extend(parse_node(node.callee.clone().into(), context));

        if let Some(type_args) = node.type_args {
            items.extend(parse_node(type_args.into(), context));
        }

        if is_optional(context) {
            items.push("?.".into());
        }

        items.push(conditions::with_indent_if_start_of_line_indented(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
            nodes: node.args.into_iter().map(|node| node.into()).collect(),
            force_multi_line_when_multiple_lines: context.config.call_expression_force_multi_line_arguments,
            custom_close_paren: None,
        }, context)).into());

        items
    }

    fn parse_test_library_call_expr(node: CallExpr, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();
        items.extend(parse_test_library_callee(&node.callee, context));
        items.extend(parse_test_library_arguments(&node.args, context));
        return items;

        fn parse_test_library_callee(callee: &ExprOrSuper, context: &mut Context) -> Vec<PrintItem> {
            match callee {
                ExprOrSuper::Expr(box Expr::Member(member_expr)) => {
                    let mut items = Vec::new();
                    items.extend(parse_node(member_expr.obj.clone().into(), context));
                    items.push(".".into());
                    items.extend(parse_node((*member_expr.prop.clone()).into(), context));
                    items
                },
                _ => parse_node(callee.clone().into(), context),
            }
        }

        fn parse_test_library_arguments(args: &Vec<ExprOrSpread>, context: &mut Context) -> Vec<PrintItem> {
            let mut items = Vec::new();
            items.push("(".into());
            items.extend(parse_node_with_inner_parse(args[0].clone().into(), context, |items| {
                // force everything to go onto one line
                let mut items = items.into_iter().filter(|item| !item.is_signal()).collect::<Vec<PrintItem>>();
                items.push(",".into());
                items
            }));
            items.push(" ".into());
            items.extend(parse_node(args[1].clone().into(), context));
            items.push(")".into());

            return items;
        }
    }

    // Tests if this is a call expression from common test libraries.
    // Be very strict here to allow the user to opt out if they'd like.
    fn is_test_library_call_expr(node: &CallExpr, context: &mut Context) -> bool {
        if node.args.len() != 2 || node.type_args.is_some() || !is_valid_callee(&node.callee) || is_optional(context) {
            return false;
        }
        if (*node.args[0].expr).kind() != NodeKind::Str && !is_expr_template(&node.args[0].expr) {
            return false;
        }
        if node.args[1].expr.kind() != NodeKind::FnExpr && node.args[1].expr.kind() != NodeKind::ArrowExpr {
            return false;
        }

        return node.start_line(context) == node.args[1].start_line(context);

        fn is_valid_callee(callee: &ExprOrSuper) -> bool {
            let ident_text = get_identifier_text(&callee);
            if let Some(ident_text) = ident_text {
                return match ident_text {
                    "it" | "describe" => true,
                    _ => false,
                };
            }
            return false;

            fn get_identifier_text(callee: &ExprOrSuper) -> Option<&str> {
                return match callee {
                    ExprOrSuper::Super(_) => None,
                    ExprOrSuper::Expr(box expr) => {
                        match expr {
                            Expr::Ident(ident) => Some(&ident.sym),
                            Expr::Member(member) if (*member.prop).kind() == NodeKind::Ident => get_identifier_text(&member.obj),
                            _ => None,
                        }
                    }
                };
            }
        }
    }

    fn is_optional(context: &Context) -> bool {
        return context.parent().kind() == NodeKind::OptChainExpr;
    }
}

fn parse_class_expr(node: ClassExpr, context: &mut Context) -> Vec<PrintItem> {
    return parse_class_decl_or_expr(ClassDeclOrExpr {
        span: node.class.span,
        decorators: node.class.decorators,
        is_class_expr: true,
        is_declare: false,
        is_abstract: node.class.is_abstract,
        ident: node.ident.map(|x| x.into()),
        type_params: node.class.type_params.map(|x| x.into()),
        super_class: node.class.super_class.map(|x| x.into()),
        super_type_params: node.class.super_type_params.map(|x| x.into()),
        implements: node.class.implements.into_iter().map(|x| x.into()).collect(),
        members: node.class.body.into_iter().map(|x| x.into()).collect(),
        brace_position: context.config.class_expression_brace_position,
    }, context);
}

fn parse_conditional_expr(node: CondExpr, context: &mut Context) -> Vec<PrintItem> {
    let operator_token = context.get_first_token_after_with_text(&node.test, "?").unwrap();
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&node.test, &node.cons, context)
        || node_helpers::get_use_new_lines_for_nodes(&node.cons, &node.alt, context);
    let operator_position = get_operator_position(&node, &operator_token, context);
    let start_info = Info::new("startConditionalExpression");
    let before_alternate_info = Info::new("beforeAlternateInfo");
    let end_info = Info::new("endConditionalExpression");
    let mut items = Vec::new();

    items.push(start_info.clone().into());
    items.extend(parser_helpers::new_line_group(parse_node_with_inner_parse(node.test.into(), context, {
        let operator_position = operator_position.clone();
        move |mut items| {
            if operator_position == OperatorPosition::SameLine {
                items.push(" ?".into());
            }
            items
        }
    })));

    // force re-evaluation of all the conditions below once the end info has been reached
    items.push(conditions::force_reevaluation_once_resolved(context.end_statement_or_member_infos.peek().unwrap_or(&end_info).clone()).into());

    if use_new_lines {
        items.push(PrintItem::NewLine);
    } else {
        items.push(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_info.clone(), Some(before_alternate_info.clone())).into());
    }

    items.push(conditions::indent_if_start_of_line({
        let mut items = Vec::new();
        if operator_position == OperatorPosition::NextLine {
            items.push("? ".into());
        }
        items.extend(parser_helpers::new_line_group(parse_node_with_inner_parse(node.cons.into(), context, {
            let operator_position = operator_position.clone();
            move |mut items| {
                if operator_position == OperatorPosition::SameLine {
                    items.push(" :".into());
                }
                items
            }
        })));
        items
    }).into());

    if use_new_lines {
        items.push(PrintItem::NewLine);
    } else {
        items.push(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_info.clone(), Some(before_alternate_info.clone())).into());
    }

    items.push(conditions::indent_if_start_of_line({
        let mut items = Vec::new();
        if operator_position == OperatorPosition::NextLine {
            items.push(": ".into());
        }
        items.push(before_alternate_info.into());
        items.extend(parser_helpers::new_line_group(parse_node(node.alt.into(), context)));
        items.push(end_info.into());
        items
    }).into());

    return items;

    fn get_operator_position(node: &CondExpr, operator_token: &TokenAndSpan, context: &mut Context) -> OperatorPosition {
        match context.config.conditional_expression_operator_position {
            OperatorPosition::NextLine => OperatorPosition::NextLine,
            OperatorPosition::SameLine => OperatorPosition::SameLine,
            OperatorPosition::Maintain => {
                if node.test.end_line(context) == operator_token.start_line(context) {
                    OperatorPosition::SameLine
                } else {
                    OperatorPosition::NextLine
                }
            }
        }
    }
}

fn parse_expr_or_spread(node: ExprOrSpread, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if node.spread.is_some() { items.push("...".into()); }
    items.extend(parse_node((*node.expr).into(), context));
    items
}

fn parse_expr_with_type_args(node: TsExprWithTypeArgs, context: &mut Context) -> Vec<PrintItem> {
    let mut vec = Vec::new();
    vec.extend(parse_node(node.expr.into(), context));
    if let Some(type_args) = node.type_args {
        vec.extend(parse_node(type_args.into(), context));
    }
    return vec;
}

fn parse_fn_expr(node: FnExpr, context: &mut Context) -> Vec<PrintItem> {
    parse_function_decl_or_expr(FunctionDeclOrExprNode {
        is_func_decl: false,
        ident: node.ident,
        declare: false,
        func: node.function,
    }, context)
}

fn parse_getter_prop(node: GetterProp, context: &mut Context) -> Vec<PrintItem> {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: Vec::new(),
        accessibility: None,
        is_static: false,
        is_async: false,
        is_abstract: false,
        kind: ClassOrObjectMethodKind::Getter,
        is_generator: false,
        is_optional: false,
        key: node.key.into(),
        type_params: None,
        params: Vec::new(),
        return_type: node.type_ann.map(|x| x.into()),
        body: node.body.map(|x| x.into()),
    }, context);
}

fn parse_key_value_prop(node: KeyValueProp, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_node(node.key.into(), context);
    items.extend(parse_node_with_preceeding_colon(Some(node.value.into()), context));
    return items;
}

fn parse_member_expr(node: MemberExpr, context: &mut Context) -> Vec<PrintItem> {
    return parse_for_member_like_expr(MemberLikeExpr {
        left_node: node.obj.into(),
        right_node: node.prop.into(),
        is_computed: node.computed,
    }, context);
}

fn parse_meta_prop_expr(node: MetaPropExpr, context: &mut Context) -> Vec<PrintItem> {
    return parse_for_member_like_expr(MemberLikeExpr {
        left_node: node.meta.into(),
        right_node: node.prop.into(),
        is_computed: false,
    }, context);
}

fn parse_new_expr(node: NewExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("new ".into());
    items.extend(parse_node((*node.callee).into(), context));
    if let Some(type_args) = node.type_args { items.extend(parse_node(type_args.into(), context)); }
    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.args.unwrap_or(Vec::new()).into_iter().map(|node| node.into()).collect(),
        force_multi_line_when_multiple_lines: context.config.new_expression_force_multi_line_arguments,
        custom_close_paren: None,
    }, context));
    return items;
}

fn parse_non_null_expr(node: TsNonNullExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_node((*node.expr).into(), context);
    items.push("!".into());
    return items;
}

fn parse_object_lit(node: ObjectLit, context: &mut Context) -> Vec<PrintItem> {
    return parse_object_like_node(ParseObjectLikeNodeOptions {
        node_span: node.span,
        members: node.props.into_iter().map(|x| x.into()).collect(),
        trailing_commas: Some(context.config.object_expression_trailing_commas),
    }, context);
}

fn parse_paren_expr(node: ParenExpr, context: &mut Context) -> Vec<PrintItem> {
    let expr = *node.expr;
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&context.get_first_open_paren_token_within(&node.span), &expr, context);
    return wrap_in_parens(parse_node(expr.into(), context), use_new_lines, context);
}

fn parse_sequence_expr(node: SeqExpr, context: &mut Context) -> Vec<PrintItem> {
    parse_comma_separated_values(node.exprs.into_iter().map(|box x| x.into()).collect(), |_| { Some(false) }, context)
}

fn parse_setter_prop(node: SetterProp, context: &mut Context) -> Vec<PrintItem> {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: Vec::new(),
        accessibility: None,
        is_static: false,
        is_async: false,
        is_abstract: false,
        kind: ClassOrObjectMethodKind::Setter,
        is_generator: false,
        is_optional: false,
        key: node.key.into(),
        type_params: None,
        params: vec![node.param.into()],
        return_type: None,
        body: node.body.map(|x| x.into()),
    }, context);
}

fn parse_spread_element(node: SpreadElement, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("...".into());
    items.extend(parse_node((*node.expr).into(), context));
    return items;
}

fn parse_tagged_tpl(node: TaggedTpl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_node((*node.tag).into(), context);
    if let Some(type_params) = node.type_params { items.extend(parse_node(type_params.into(), context)); }
    items.push(PrintItem::SpaceOrNewLine);
    items.push(conditions::indent_if_start_of_line(parse_template_literal(node.quasis, node.exprs.into_iter().map(|box x| x).collect())).into());
    return items;

    fn parse_template_literal(quasis: Vec<TplElement>, exprs: Vec<Expr>) -> Vec<PrintItem> {
        vec![] // todo
    }
}

fn parse_type_assertion(node: TsTypeAssertion, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("<".into());
    items.extend(parse_node((*node.type_ann).into(), context));
    items.push(">".into());
    if context.config.type_assertion_space_before_expression { items.push(" ".into()); }
    items.extend(parse_node((*node.expr).into(), context));
    items
}

fn parse_unary_expr(node: UnaryExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.insert(0, get_operator_text(node.op).into());
    items.extend(parse_node((*node.arg).into(), context));
    return items;

    fn get_operator_text<'a>(op: UnaryOp) -> &'a str {
        match op {
            UnaryOp::Void => "void ",
            UnaryOp::TypeOf => "typeof ",
            UnaryOp::Delete => "delete ",
            UnaryOp::Bang => "!",
            UnaryOp::Plus => "+",
            UnaryOp::Minus => "-",
            UnaryOp::Tilde => "~",
        }
    }
}

fn parse_update_expr(node: UpdateExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_node((*node.arg).into(), context);
    let operator_text = get_operator_text(node.op);
    if node.prefix {
        items.insert(0, operator_text.into());
    } else {
        items.push(operator_text.into());
    }
    return items;

    fn get_operator_text<'a>(operator: UpdateOp) -> &'a str {
        match operator {
            UpdateOp::MinusMinus => "--",
            UpdateOp::PlusPlus => "++",
        }
    }
}

fn parse_yield_expr(node: YieldExpr, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("yield".into());
    if node.delegate { items.push("*".into()); }
    if let Some(box arg) = node.arg {
        items.push(" ".into());
        items.extend(parse_node(arg.into(), context));
    }
    items
}

/* exports */

fn parse_export_named_specifier(node: NamedExportSpecifier, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    items.extend(parse_node(node.orig.into(), context));
    if let Some(exported) = node.exported {
        items.push(PrintItem::SpaceOrNewLine);
        items.push(conditions::indent_if_start_of_line({
            let mut items = Vec::new();
            items.push("as ".into());
            items.extend(parse_node(exported.into(), context));
            items
        }).into());
    }

    items
}

/* imports */

fn parse_import_named_specifier(node: ImportSpecific, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    if let Some(imported) = node.imported {
        items.extend(parse_node(imported.into(), context));
        items.push(PrintItem::SpaceOrNewLine);
        items.push(conditions::indent_if_start_of_line({
            let mut items = Vec::new();
            items.push("as ".into());
            items.extend(parse_node(node.local.into(), context));
            items
        }).into());
    } else {
        items.extend(parse_node(node.local.into(), context));
    }

    items
}

fn parse_import_namespace_specifier(node: ImportStarAs, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("* as ".into());
    items.extend(parse_node(node.local.into(), context));
    return items;
}

fn parse_external_module_ref(node: TsExternalModuleRef, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("require".into());
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&context.get_first_open_paren_token_within(&node.span), &node.expr, context);
    items.extend(wrap_in_parens(parse_node(node.expr.into(), context), use_new_lines, context));
    return items;
}

/* interface / type element */

fn parse_call_signature_decl(node: TsCallSignatureDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let start_info = Info::new("startCallSignature");

    items.push(start_info.clone().into());
    if let Some(type_params) = node.type_params { items.extend(parse_node(type_params.into(), context)); }
    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.into_iter().map(|node| node.into()).collect(),
        force_multi_line_when_multiple_lines: context.config.call_signature_force_multi_line_parameters,
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info,
            type_node: node.type_ann.map(|x| x.into()),
            type_node_separator: None,
        }, context)),
    }, context));
    if context.config.call_signature_semi_colon { items.push(";".into()); }

    return items;
}

fn parse_construct_signature_decl(node: TsConstructSignatureDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let start_info = Info::new("startConstructSignature");

    items.push(start_info.clone().into());
    items.push("new".into());
    if context.config.construct_signature_space_after_new_keyword { items.push(" ".into()); }
    if let Some(type_params) = node.type_params { items.extend(parse_node(type_params.into(), context)); }
    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.into_iter().map(|node| node.into()).collect(),
        force_multi_line_when_multiple_lines: context.config.construct_signature_force_multi_line_parameters,
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info,
            type_node: node.type_ann.map(|x| x.into()),
            type_node_separator: None,
        }, context)),
    }, context));
    if context.config.construct_signature_semi_colon { items.push(";".into()); }

    return items;
}

fn parse_index_signature(node: TsIndexSignature, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    if node.readonly { items.push("readonly ".into()); }

    // todo: this should do something similar to the other declarations here (the ones with customCloseParen)
    items.push("[".into());
    items.extend(parse_node(node.params.into_iter().next().expect("Expected the index signature to have one parameter.").into(), context));
    items.push("]".into());
    items.extend(parse_type_annotation_with_colon_if_exists(node.type_ann, context));
    if context.config.index_signature_semi_colon { items.push(";".into()); }

    return items;
}

fn parse_interface_body(node: TsInterfaceBody, context: &mut Context) -> Vec<PrintItem> {
    let start_header_info = get_parent_info(context);

    return parse_membered_body(ParseMemberedBodyOptions {
        span: node.span,
        members: node.body.into_iter().map(|x| x.into()).collect(),
        start_header_info: start_header_info,
        brace_position: context.config.interface_declaration_brace_position,
        should_use_blank_line: Box::new(move |previous, next, context| {
            node_helpers::has_separating_blank_line(previous, next, context)
        }),
        trailing_commas: None,
    }, context);

    fn get_parent_info(context: &mut Context) -> Option<Info> {
        for ancestor in context.parent_stack.iter().rev() {
            if let Node::TsInterfaceDecl(ancestor) = ancestor {
                return context.get_info_for_node(&ancestor).map(|x| x.to_owned());
            }
        }
        return None;
    }
}

fn parse_method_signature(node: TsMethodSignature, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let start_info = Info::new("startMethodSignature");
    items.push(start_info.clone().into());

    if node.computed { items.push("[".into()); }
    items.extend(parse_node(node.key.into(), context));
    if node.computed { items.push("]".into()); }
    if node.optional { items.push("?".into()); }
    if let Some(type_params) = node.type_params { items.extend(parse_node(type_params.into(), context)); }

    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.into_iter().map(|node| node.into()).collect(),
        force_multi_line_when_multiple_lines: context.config.method_signature_force_multi_line_parameters,
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info,
            type_node: node.type_ann.map(|x| x.into()),
            type_node_separator: None,
        }, context)),
    }, context));

    if context.config.method_signature_semi_colon { items.push(";".into()); }

    return items;
}

fn parse_property_signature(node: TsPropertySignature, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if node.readonly { items.push("readonly ".into()); }
    if node.computed { items.push("[".into()); }
    items.extend(parse_node(node.key.into(), context));
    if node.computed { items.push("]".into()); }
    if node.optional { items.push("?".into()); }
    items.extend(parse_type_annotation_with_colon_if_exists(node.type_ann, context));

    if let Some(init) = node.init {
        items.push(PrintItem::SpaceOrNewLine);
        items.push(conditions::indent_if_start_of_line({
            let mut items = Vec::new();
            items.push("= ".into());
            items.extend(parse_node(init.into(), context));
            items
        }).into());
    }

    if context.config.property_signature_semi_colon { items.push(";".into()); }

    return items;
}

fn parse_type_lit(node: TsTypeLit, context: &mut Context) -> Vec<PrintItem> {
    return parse_object_like_node(ParseObjectLikeNodeOptions {
        node_span: node.span,
        members: node.members.into_iter().map(|m| m.into()).collect(),
        trailing_commas: None
    }, context);
}

/* literals */

fn parse_big_int_literal(node: BigInt, context: &mut Context) -> Vec<PrintItem> {
    vec![node.text(context).into()]
}

fn parse_bool_literal(node: Bool) -> Vec<PrintItem> {
    vec![match node.value {
        true => "true",
        false => "false",
    }.into()]
}

fn parse_jsx_text(node: JSXText, context: &mut Context) -> Vec<PrintItem> {
    vec![]
}

fn parse_num_literal(node: Number, context: &mut Context) -> Vec<PrintItem> {
    vec![node.text(context).into()]
}

fn parse_reg_exp_literal(node: Regex, _: &mut Context) -> Vec<PrintItem> {
    // the exp and flags should not be nodes so just ignore that (swc issue #511)
    let mut items = Vec::new();
    items.push("/".into());
    items.push((&node.exp as &str).into());
    items.push("/".into());
    items.push((&node.flags as &str).into());
    items
}

fn parse_string_literal(node: Str, context: &mut Context) -> Vec<PrintItem> {
    return parse_raw_string(&get_string_literal_text(get_string_value(&node, context), context));

    fn get_string_literal_text(string_value: String, context: &mut Context) -> String {
        match context.config.single_quotes {
            true => format!("'{}'", string_value.replace("'", "\\'")),
            false => format!("\"{}\"", string_value.replace("\"", "\\\"")),
        }
    }

    fn get_string_value(node: &Str, context: &mut Context) -> String {
        let raw_string_text = node.text(context);
        let string_value = raw_string_text.chars().skip(1).take(raw_string_text.chars().count() - 2).collect::<String>();
        let is_double_quote = raw_string_text.chars().next().unwrap() == '"';

        match is_double_quote {
            true => string_value.replace("\\\"", "\""),
            false => string_value.replace("\\'", "'"),
        }
    }
}

/* module */

fn parse_module(node: Module, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if let Some(shebang) = node.shebang {
        items.push("#!".into());
        items.push((&shebang as &str).into());
        items.push(PrintItem::NewLine);
        if let Some(first_statement) = node.body.first() {
            if node_helpers::has_separating_blank_line(&node.span.lo(), &first_statement, context) {
                items.push(PrintItem::NewLine);
            }
        }
    }
    items.extend(parse_statements_or_members(ParseStatementsOrMembersOptions {
        inner_span: node.span,
        items: node.body.into_iter().map(|module_item| (module_item.into(), None)).collect(),
        should_use_space: None,
        should_use_new_line: None,
        should_use_blank_line: Box::new(|previous, next, context| node_helpers::has_separating_blank_line(previous, next, context)),
        trailing_commas: None,
    }, context));
    return items;
}

/* patterns */

fn parse_array_pat(node: ArrayPat, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_array_like_nodes(ParseArrayLikeNodesOptions {
        node: node.clone().into(),
        elements: node.elems.into_iter().map(|x| x.map(|elem| elem.into())).collect(),
        trailing_commas: context.config.array_pattern_trailing_commas,
    }, context);
    items.extend(parse_type_annotation_with_colon_if_exists(node.type_ann, context));
    items
}

fn parse_assign_pat(node: AssignPat, context: &mut Context) -> Vec<PrintItem> {
    parser_helpers::new_line_group({
        let mut items = parse_node((*node.left).into(), context);
        items.push(PrintItem::SpaceOrNewLine);
        items.push(conditions::indent_if_start_of_line({
            let mut items = vec!["= ".into()];
            items.extend(parse_node((*node.right).into(), context));
            items
        }).into());
        items
    })
}

fn parse_assign_pat_prop(node: AssignPatProp, context: &mut Context) -> Vec<PrintItem> {
    return parser_helpers::new_line_group({
        let mut items = Vec::new();
        items.extend(parse_node(node.key.into(), context));
        if let Some(box value) = node.value {
            items.push(PrintItem::SpaceOrNewLine);
            items.push(conditions::indent_if_start_of_line({
                let mut items = Vec::new();
                items.push("= ".into());
                items.extend(parse_node(value.into(), context));
                items
            }).into());
        }
        items
    });
}

fn parse_rest_pat(node: RestPat, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("...".into());
    items.extend(parse_node((*node.arg).into(), context));
    items.extend(parse_type_annotation_with_colon_if_exists(node.type_ann, context));
    items
}

fn parse_object_pattern(node: ObjectPat, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_object_like_node(ParseObjectLikeNodeOptions {
        node_span: node.span,
        members: node.props.into_iter().map(|x| x.into()).collect(),
        trailing_commas: Some(TrailingCommas::Never),
    }, context);
    if let Some(type_ann) = node.type_ann {
        items.extend(parse_node(type_ann.into(), context));
    }
    return items;
}

/* properties */

fn parse_method_prop(node: MethodProp, context: &mut Context) -> Vec<PrintItem> {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: Vec::new(),
        accessibility: None,
        is_static: false,
        is_async: node.function.is_async,
        is_abstract: false,
        kind: ClassOrObjectMethodKind::Method,
        is_generator: node.function.is_generator,
        is_optional: false,
        key: node.key.into(),
        type_params: node.function.type_params.map(|x| x.into()),
        params: node.function.params.into_iter().map(|x| x.into()).collect(),
        return_type: node.function.return_type.map(|x| x.into()),
        body: node.function.body.map(|x| x.into()),
    }, context);
}

struct ClassOrObjectMethod {
    decorators: Vec<Decorator>,
    accessibility: Option<Accessibility>,
    is_static: bool,
    is_async: bool,
    is_abstract: bool,
    kind: ClassOrObjectMethodKind,
    is_generator: bool,
    is_optional: bool,
    key: Node,
    type_params: Option<Node>,
    params: Vec<Node>,
    return_type: Option<Node>,
    body: Option<Node>,
}

enum ClassOrObjectMethodKind {
    Getter,
    Setter,
    Method,
    Constructor,
}

impl From<MethodKind> for ClassOrObjectMethodKind {
    fn from(kind: MethodKind) -> ClassOrObjectMethodKind {
        match kind {
            MethodKind::Getter => ClassOrObjectMethodKind::Getter,
            MethodKind::Setter => ClassOrObjectMethodKind::Setter,
            MethodKind::Method => ClassOrObjectMethodKind::Method,
        }
    }
}

fn parse_class_or_object_method(node: ClassOrObjectMethod, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.extend(parse_decorators(node.decorators, false, context));

    let start_header_info = Info::new("methodStartHeaderInfo");
    items.push(start_header_info.clone().into());

    if let Some(accessibility) = node.accessibility {
        items.push(format!("{} ", accessibility_to_str(&accessibility)).into());
    }
    if node.is_static { items.push("static ".into()); }
    if node.is_async { items.push("async ".into()); }
    if node.is_abstract { items.push("abstract ".into()); }

    match node.kind {
        ClassOrObjectMethodKind::Getter => items.push("get ".into()),
        ClassOrObjectMethodKind::Setter => items.push("set ".into()),
        ClassOrObjectMethodKind::Method | ClassOrObjectMethodKind::Constructor => {},
    }

    if node.is_generator { items.push("*".into()); }
    items.extend(parse_node(node.key, context));
    if node.is_optional { items.push("?".into()); }
    if let Some(type_params) = node.type_params { items.extend(parse_node(type_params, context)); }
    if get_use_space_before_parens(&node.kind, context) { items.push(" ".into()) }

    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.into_iter().map(|node| node.into()).collect(),
        force_multi_line_when_multiple_lines: get_force_multi_line_parameters(&node.kind, context),
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info: start_header_info.clone(),
            type_node: node.return_type,
            type_node_separator: None,
        }, context)),
    }, context));

    if let Some(body) = node.body {
        let brace_position = get_brace_position(&node.kind, context);
        items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
            brace_position: brace_position,
            open_brace_token: &context.get_first_open_brace_token_within(&body),
            start_header_info: Some(start_header_info),
        }, context));
        items.extend(parse_node(body, context));
    } else if get_use_semi_colon(&node.kind, context) {
        items.push(";".into());
    }

    return items;

    fn get_force_multi_line_parameters(kind: &ClassOrObjectMethodKind, context: &mut Context) -> bool {
        match kind {
            ClassOrObjectMethodKind::Constructor => context.config.constructor_force_multi_line_parameters,
            ClassOrObjectMethodKind::Getter => context.config.get_accessor_force_multi_line_parameters,
            ClassOrObjectMethodKind::Setter => context.config.set_accessor_force_multi_line_parameters,
            ClassOrObjectMethodKind::Method => context.config.method_force_multi_line_parameters,
        }
    }

    fn get_use_space_before_parens(kind: &ClassOrObjectMethodKind, context: &mut Context) -> bool {
        match kind {
            ClassOrObjectMethodKind::Constructor => context.config.constructor_space_before_parentheses,
            ClassOrObjectMethodKind::Getter => context.config.get_accessor_space_before_parentheses,
            ClassOrObjectMethodKind::Setter => context.config.set_accessor_space_before_parentheses,
            ClassOrObjectMethodKind::Method => context.config.method_space_before_parentheses,
        }
    }

    fn get_brace_position(kind: &ClassOrObjectMethodKind, context: &mut Context) -> BracePosition {
        match kind {
            ClassOrObjectMethodKind::Constructor => context.config.constructor_brace_position,
            ClassOrObjectMethodKind::Getter => context.config.get_accessor_brace_position,
            ClassOrObjectMethodKind::Setter => context.config.set_accessor_brace_position,
            ClassOrObjectMethodKind::Method => context.config.method_brace_position,
        }
    }

    fn get_use_semi_colon(kind: &ClassOrObjectMethodKind, context: &mut Context) -> bool {
        match kind {
            ClassOrObjectMethodKind::Constructor => context.config.constructor_semi_colon,
            ClassOrObjectMethodKind::Getter => context.config.get_accessor_semi_colon,
            ClassOrObjectMethodKind::Setter => context.config.set_accessor_semi_colon,
            ClassOrObjectMethodKind::Method => context.config.method_semi_colon,
        }
    }
}

fn accessibility_to_str(accessibility: &Accessibility) -> &str {
    match accessibility {
        Accessibility::Private => "private",
        Accessibility::Protected => "protected",
        Accessibility::Public => "public",
    }
}

/* statements */

fn parse_block_stmt(node: BlockStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let start_statements_info = Info::new("startStatementsInfo");
    let end_statements_info = Info::new("endStatementsInfo");

    items.push("{".into());

    // todo: inner comments

    let is_arrow_or_fn_expr = match context.parent().kind() { NodeKind::ArrowExpr | NodeKind::FnExpr => true, _ => false };
    // todo: inner comments on this condition
    if is_arrow_or_fn_expr && node.start_line(context) == node.end_line(context) && node.stmts.is_empty() && node.leading_comments(context).is_empty() {
        items.push("}".into());
        return items;
    }

    items.extend(parse_first_line_trailing_comments(&node, node.stmts.get(0).map(|x| x as &dyn Spanned), context));
    items.push(PrintItem::NewLine);
    items.push(start_statements_info.clone().into());
    items.extend(parser_helpers::with_indent(
        parse_statements(node.get_inner_span(context), node.stmts.into_iter().map(|stmt| stmt.into()).collect(), context)
    ));
    items.push(end_statements_info.clone().into());
    items.push(Condition::new("endStatementsNewLine", ConditionProperties {
        condition: Box::new(move |context| {
            condition_resolvers::are_infos_equal(context, &start_statements_info, &end_statements_info)
        }),
        true_path: None,
        false_path: Some(vec![PrintItem::NewLine]),
    }).into());
    items.push("}".into());

    return items;
}

fn parse_break_stmt(node: BreakStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    items.push("break".into());
    if let Some(label) = node.label {
        items.push(" ".into());
        items.extend(parse_node(label.into(), context));
    }
    if context.config.break_statement_semi_colon {
        items.push(";".into());
    }

    items
}

fn parse_continue_stmt(node: ContinueStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    items.push("continue".into());
    if let Some(label) = node.label {
        items.push(" ".into());
        items.extend(parse_node(label.into(), context));
    }
    if context.config.continue_statement_semi_colon {
        items.push(";".into());
    }

    items
}

fn parse_debugger_stmt(_: DebuggerStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    items.push("debugger".into());
    if context.config.debugger_statement_semi_colon {
        items.push(";".into());
    }

    items
}

fn parse_do_while_stmt(node: DoWhileStmt, context: &mut Context) -> Vec<PrintItem> {
    // the braces are technically optional on do while statements
    let mut items = Vec::new();
    items.push("do".into());
    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: context.config.do_while_statement_brace_position,
        open_brace_token: &if let Stmt::Block(_) = &*node.body { context.get_first_open_brace_token_within(&node) } else { None },
        start_header_info: None,
    }, context));
    items.extend(parse_node(node.body.into(), context));
    items.push(" while".into());
    if context.config.do_while_statement_space_after_while_keyword {
        items.push(" ".into());
    }
    let test_span = &(*node.test).span();
    items.extend(parse_node_in_parens(test_span, parse_node(node.test.into(), context), context));
    if context.config.do_while_statement_semi_colon {
        items.push(";".into());
    }
    return items;
}

fn parse_export_all(node: ExportAll, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("export * from ".into());
    items.extend(parse_node(node.src.into(), context));

    if context.config.export_all_declaration_semi_colon {
        items.push(";".into());
    }

    items
}

fn parse_empty_stmt(_: EmptyStmt, _: &mut Context) -> Vec<PrintItem> {
    // Don't have configuration for this. Perhaps a change here would be
    // to not print anything for empty statements?
    vec![";".into()]
}

fn parse_export_assignment(node: TsExportAssignment, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    items.push("export = ".into());
    items.extend(parse_node((*node.expr).into(), context));
    if context.config.export_assignment_semi_colon {
        items.push(";".into());
    }

    items
}

fn parse_namespace_export(node: TsNamespaceExportDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("export as namespace ".into());
    items.extend(parse_node(node.id.into(), context));

    if context.config.namespace_export_declaration_semi_colon {
        items.push(";".into());
    }

    items
}

fn parse_expr_stmt(stmt: ExprStmt, context: &mut Context) -> Vec<PrintItem> {
    if context.config.expression_statement_semi_colon {
        return parse_inner(&stmt, context);
    } else {
        return parse_for_prefix_semi_colon_insertion(&stmt, context);
    }

    fn parse_inner(stmt: &ExprStmt, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();
        items.extend(parse_node((*stmt.expr.clone()).into(), context));
        if context.config.expression_statement_semi_colon {
            items.push(";".into());
        }
        return items;
    }

    fn parse_for_prefix_semi_colon_insertion(stmt: &ExprStmt, context: &mut Context) -> Vec<PrintItem> {
        let mut parsed_node = parse_inner(&stmt, context);
        if should_add_semi_colon(&parsed_node).unwrap_or(false) {
            parsed_node.insert(0, ";".into());
        }
        return parsed_node;

        fn should_add_semi_colon(items: &Vec<PrintItem>) -> Option<bool> {
            for item in items {
                match item {
                    PrintItem::String(value) => {
                        if let Some(c) = value.chars().next() {
                            return utils::is_prefix_semi_colon_insertion_char(c).into();
                        }
                    },
                    PrintItem::Condition(condition) => {
                        // It's an assumption here that th etrue and false paths of the
                        // condition will both contain the same text to look for.
                        if let Some(true_path) = &condition.true_path {
                            if let Some(result) = should_add_semi_colon(&true_path) {
                                return result.into();
                            }
                        }
                        if let Some(false_path) = &condition.false_path {
                            if let Some(result) = should_add_semi_colon(&false_path) {
                                return result.into();
                            }
                        }
                    },
                    _ => { /* do nothing */ },
                }
            }

            None
        }
    }
}

fn parse_for_stmt(node: ForStmt, context: &mut Context) -> Vec<PrintItem> {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = Vec::new();
    items.push(start_header_info.clone().into());
    items.push("for".into());
    if context.config.for_statement_space_after_for_keyword {
        items.push(" ".into());
    }
    items.extend(parse_node_in_parens(&{
        if let Some(init) = &node.init {
            init.span()
        } else {
            context.get_first_semi_colon_within(&node).expect("Expected to find a semi-colon within the for stmt.").span()
        }
    }, {
        let mut items = Vec::new();
        let separator_after_semi_colons = if context.config.for_statement_space_after_semi_colons { PrintItem::SpaceOrNewLine } else { PrintItem::PossibleNewLine };
        items.extend(parser_helpers::new_line_group({
            let mut items = Vec::new();
            if let Some(init) = node.init {
                items.extend(parse_node(init.into(), context));
            }
            items.push(";".into());
            items
        }));
        items.push(separator_after_semi_colons.clone());
        items.push(conditions::indent_if_start_of_line({
            let mut items = Vec::new();
            if let Some(test) = node.test {
                items.extend(parse_node(test.into(), context));
            }
            items.push(";".into());
            items
        }).into());
        items.push(separator_after_semi_colons.clone());
        items.push(conditions::indent_if_start_of_line({
            if let Some(update) = node.update {
                parse_node(update.into(), context)
            } else {
                vec![]
            }
        }).into());
        items
    }, context));
    items.push(end_header_info.clone().into());

    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: node.body.into(),
        use_braces: context.config.for_statement_use_braces,
        brace_position: context.config.for_statement_brace_position,
        single_body_position: Some(context.config.for_statement_single_body_position),
        requires_braces_condition: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);

    return items;
}

fn parse_for_in_stmt(node: ForInStmt, context: &mut Context) -> Vec<PrintItem> {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = Vec::new();
    items.push(start_header_info.clone().into());
    items.push("for".into());
    if context.config.for_in_statement_space_after_for_keyword {
        items.push(" ".into());
    }
    let left_span = node.left.span();
    items.extend(parse_node_in_parens(&left_span, {
        let mut items = Vec::new();
        items.extend(parse_node(node.left.into(), context));
        items.push(PrintItem::SpaceOrNewLine);
        items.push(conditions::indent_if_start_of_line({
            let mut items = Vec::new();
            items.push("in ".into());
            items.extend(parse_node(node.right.into(), context));
            items
        }).into());
        items
    }, context));
    items.push(end_header_info.clone().into());

    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: node.body.into(),
        use_braces: context.config.for_in_statement_use_braces,
        brace_position: context.config.for_in_statement_brace_position,
        single_body_position: Some(context.config.for_in_statement_single_body_position),
        requires_braces_condition: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);

    return items;
}

fn parse_for_of_stmt(node: ForOfStmt, context: &mut Context) -> Vec<PrintItem> {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = Vec::new();
    items.push(start_header_info.clone().into());
    items.push("for".into());
    if context.config.for_of_statement_space_after_for_keyword {
        items.push(" ".into());
    }
    if let Some(await_token) = node.await_token {
        items.extend(parse_node(await_token.into(), context));
        items.push(" ".into());
    }
    let left_span = node.left.span();
    items.extend(parse_node_in_parens(&left_span, {
        let mut items = Vec::new();
        items.extend(parse_node(node.left.into(), context));
        items.push(PrintItem::SpaceOrNewLine);
        items.push(conditions::indent_if_start_of_line({
            let mut items = Vec::new();
            items.push("of ".into());
            items.extend(parse_node(node.right.into(), context));
            items
        }).into());
        items
    }, context));
    items.push(end_header_info.clone().into());

    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: node.body.into(),
        use_braces: context.config.for_of_statement_use_braces,
        brace_position: context.config.for_of_statement_brace_position,
        single_body_position: Some(context.config.for_of_statement_single_body_position),
        requires_braces_condition: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);

    return items;
}

fn parse_if_stmt(node: IfStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let result = parse_header_with_conditional_brace_body(ParseHeaderWithConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: node.cons.into(),
        parsed_header: {
            let mut items = Vec::new();
            items.push("if".into());
            if context.config.if_statement_space_after_if_keyword { items.push(" ".into()); }
            let test = *node.test;
            let test_span = test.span();
            items.extend(parse_node_in_parens(&test_span, parse_node(test.into(), context), context));
            items
        },
        use_braces: context.config.if_statement_use_braces,
        brace_position: context.config.if_statement_brace_position,
        single_body_position: Some(context.config.if_statement_single_body_position),
        requires_braces_condition: None,
    }, context);

    items.extend(result.parsed_node);

    if let Some(box alt) = node.alt {
        items.extend(parse_control_flow_separator(context.config.if_statement_next_control_flow_position, &alt.span(), "else", context));

        // parse the leading comments before the else keyword
        let else_keyword = context.get_first_else_keyword_before(&alt).expect("Expected to find an else keyword.");
        items.extend(parse_leading_comments(&else_keyword, context));
        items.extend(parse_leading_comments(&alt, context));

        let start_else_header_info = Info::new("startElseHeader");
        items.push(start_else_header_info.clone().into());
        items.push("else".into());

        if let Stmt::If(alt) = alt {
            items.push(" ".into());
            items.extend(parse_node(alt.into(), context));
        } else {
            let alt_span = alt.span();
            items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
                parent: &node.span,
                body_node: alt.into(),
                use_braces: context.config.if_statement_use_braces,
                brace_position: context.config.if_statement_brace_position,
                single_body_position: Some(context.config.if_statement_single_body_position),
                requires_braces_condition: Some(result.open_brace_condition),
                header_start_token: context.get_first_token_before_with_text(&alt_span, "else"),
                start_header_info: Some(start_else_header_info),
                end_header_info: None,
            }, context).parsed_node);
        }
    }

    return items;
}

fn parse_labeled_stmt(node: LabeledStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.extend(parse_node(node.label.into(), context));
    items.push(":".into());

    // not bothering to make this configurable, because who uses labeled statements?
    items.push(if node.body.kind() == NodeKind::BlockStmt {
        " ".into()
    } else {
        PrintItem::NewLine
    });

    items.extend(parse_node(node.body.into(), context));

    return items;
}

fn parse_return_stmt(node: ReturnStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("return".into());
    if let Some(box arg) = node.arg {
        items.push(" ".into());
        items.extend(parse_node(arg.into(), context));
    }
    if context.config.return_statement_semi_colon { items.push(";".into()); }
    return items;
}

fn parse_switch_stmt(node: SwitchStmt, context: &mut Context) -> Vec<PrintItem> {
    let start_header_info = Info::new("startHeader");
    let mut items = Vec::new();
    items.push(start_header_info.clone().into());
    items.push("switch ".into());
    let discriminate_span = node.discriminant.span();
    items.extend(parse_node_in_parens(&discriminate_span, parse_node(node.discriminant.into(), context), context));
    items.extend(parse_membered_body(ParseMemberedBodyOptions {
        span: node.span,
        members: node.cases.into_iter().map(|x| x.into()).collect(),
        start_header_info: Some(start_header_info),
        brace_position: context.config.switch_statement_brace_position,
        should_use_blank_line: Box::new(|_, _, _| false),
        trailing_commas: None,
    }, context));
    return items;
}

fn parse_switch_case(node: SwitchCase, context: &mut Context) -> Vec<PrintItem> {
    let block_stmt_body = get_block_stmt_body(&node);
    let start_header_info = Info::new("switchCaseStartHeader");
    let mut items = Vec::new();
    let colon_token = context.get_first_colon_token_after(&if let Some(test) = &node.test {
        test.span().hi()
    } else {
        node.span.lo()
    }).expect("Expected to find a colon token.");

    items.push(start_header_info.clone().into());

    if let Some(box test) = node.test {
        items.push("case ".into());
        items.extend(parse_node(test.into(), context));
        items.push(":".into());
    } else {
        items.push("default:".into());
    }

    items.extend(parse_first_line_trailing_comments(&node.span, node.cons.get(0).map(|x| x as &dyn Spanned), context));
    let parsed_trailing_comments = parse_trailing_comments_for_case(node.span, &block_stmt_body, context);
    if !node.cons.is_empty() {
        if let Some(block_stmt_body) = block_stmt_body {
            items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
                brace_position: context.config.switch_case_brace_position,
                open_brace_token: &context.get_first_open_brace_token_within(&block_stmt_body),
                start_header_info: None,
            }, context));
            items.extend(parse_node(node.cons.into_iter().next().unwrap().into(), context));
        } else {
            items.push(PrintItem::NewLine);
            items.extend(parser_helpers::with_indent(parse_statements_or_members(ParseStatementsOrMembersOptions {
                inner_span: Span::new(colon_token.hi(), node.span.hi(), Default::default()),
                items: node.cons.into_iter().map(|node| (node.into(), None)).collect(),
                should_use_space: None,
                should_use_new_line: None,
                should_use_blank_line: Box::new(|previous, next, context| node_helpers::has_separating_blank_line(previous, next, context)),
                trailing_commas: None,
            }, context)));
        }
    }

    items.extend(parsed_trailing_comments);

    return items;

    fn get_block_stmt_body(node: &SwitchCase) -> Option<Span> {
        let first_cons = node.cons.get(0);
        if let Some(Stmt::Block(block_stmt)) = first_cons {
            if node.cons.len() == 1 {
                return Some(block_stmt.span);
            }
        }
        return None;
    }

    fn parse_trailing_comments_for_case(node_span: Span, block_stmt_body: &Option<Span>, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();
        // parse the trailing comments as statements
        let trailing_comments = get_trailing_comments_as_statements(&node_span, context);
        if !trailing_comments.is_empty() {
            if let Node::SwitchStmt(stmt) = context.parent() {
                let last_case = stmt.cases.iter().last();
                let is_last_case = match last_case { Some(last_case) => last_case.lo() == node_span.lo(), _=> false };
                let mut is_equal_indent = block_stmt_body.is_some();
                let mut last_node = node_span;

                for comment in trailing_comments {
                    is_equal_indent = is_equal_indent || comment.start_column(context) <= last_node.start_column(context);
                    let parsed_comment = parse_comment_based_on_last_node(&comment, &Some(&last_node), context);

                    items.extend(if !is_last_case && is_equal_indent {
                        parsed_comment
                    } else {
                        parser_helpers::with_indent(parsed_comment)
                    });
                    last_node = comment.span;
                }
            }
        }
        return items;
    }
}

fn parse_throw_stmt(node: ThrowStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("throw ".into());
    items.extend(parse_node((*node.arg).into(), context));
    if context.config.throw_statement_semi_colon { items.push(";".into()); }
    return items;
}

fn parse_try_stmt(node: TryStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let brace_position = context.config.try_statement_brace_position;
    let next_control_flow_position = context.config.try_statement_next_control_flow_position;

    items.push("try".into());
    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: brace_position,
        open_brace_token: &context.get_first_open_brace_token_within(&node.block),
        start_header_info: None,
    }, context));
    items.extend(parse_node(node.block.into(), context));

    if let Some(handler) = node.handler {
        items.extend(parse_control_flow_separator(next_control_flow_position, &handler.span, "catch", context));
        items.extend(parse_node(handler.into(), context));
    }

    if let Some(finalizer) = node.finalizer {
        items.extend(parse_control_flow_separator(next_control_flow_position, &finalizer.span, "finally", context));
        items.push("finally".into());
        items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
            brace_position: brace_position,
            open_brace_token: &context.get_first_open_brace_token_within(&finalizer),
            start_header_info: None,
        }, context));
        items.extend(parse_node(finalizer.into(), context));
    }

    return items;
}

fn parse_var_decl(node: VarDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if node.declare { items.push("declare ".into()); }
    items.push(match node.kind {
        VarDeclKind::Const => "const ",
        VarDeclKind::Let => "let ",
        VarDeclKind::Var => "var ",
    }.into());

    for (i, decl) in node.decls.into_iter().enumerate() {
        if i > 0 {
            items.push(",".into());
            items.push(PrintItem::SpaceOrNewLine);
        }

        items.push(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node(decl.into(), context))).into());
    }

    if requires_semi_colon(&node.span, context) { items.push(";".into()); }

    return items;

    fn requires_semi_colon(var_decl_span: &Span, context: &mut Context) -> bool {
        let parent = context.parent();
        match parent {
            Node::ForInStmt(node) => var_decl_span.lo() >= node.body.span().lo(),
            Node::ForOfStmt(node) => var_decl_span.lo() >= node.body.span().lo(),
            Node::ForStmt(node) => var_decl_span.lo() >= node.body.span().lo(),
            _ => context.config.variable_statement_semi_colon,
        }
    }
}

fn parse_var_declarator(node: VarDeclarator, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_node(node.name.into(), context);

    if let Some(box init) = node.init {
        items.push(" = ".into());
        items.extend(parse_node(init.into(), context));
    }

    items
}

fn parse_while_stmt(node: WhileStmt, context: &mut Context) -> Vec<PrintItem> {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = Vec::new();
    items.push(start_header_info.clone().into());
    items.push("while".into());
    if context.config.while_statement_space_after_while_keyword {
        items.push(" ".into());
    }
    let test_span = &node.test.span();
    items.extend(parse_node_in_parens(test_span, parse_node(node.test.into(), context), context));
    items.push(end_header_info.clone().into());
    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: node.body.into(),
        use_braces: context.config.while_statement_use_braces,
        brace_position: context.config.while_statement_brace_position,
        single_body_position: Some(context.config.while_statement_single_body_position),
        requires_braces_condition: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);
    return items;
}

/* types */

fn parse_array_type(node: TsArrayType, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.extend(parse_node(node.elem_type.into(), context));
    items.push("[]".into());
    return items;
}

fn parse_import_type(node: TsImportType, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    items.push("import(".into());
    items.extend(parse_node(node.arg.into(), context));
    items.push(")".into());

    if let Some(qualifier) = node.qualifier {
        items.push(".".into());
        items.extend(parse_node(qualifier.into(), context));
    }

    if let Some(type_args) = node.type_args {
        items.extend(parse_node(type_args.into(), context));
    }
    return items;
}

fn parse_lit_type(node: TsLitType, context: &mut Context) -> Vec<PrintItem> {
    parse_node(node.lit.into(), context)
}

fn parse_type_ann(node: TsTypeAnn, context: &mut Context) -> Vec<PrintItem> {
    parse_node((*node.type_ann).into(), context)
}

fn parse_type_param_instantiation(node: TypeParamNode, context: &mut Context) -> Vec<PrintItem> {
    let params = node.params();
    let use_new_lines = get_use_new_lines(&params, context);
    let parsed_params = parse_parameter_list(params, use_new_lines, context);
    let mut items = Vec::new();

    items.push("<".into());
    items.extend(if use_new_lines {
        parser_helpers::surround_with_new_lines(parsed_params)
    } else {
        parsed_params
    });
    items.push(">".into());

    return items;

    fn parse_parameter_list(params: Vec<Node>, use_new_lines: bool, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();
        let params_count = params.len();

        for (i, param) in params.into_iter().enumerate() {
            if i > 0 {
                items.push(if use_new_lines { PrintItem::NewLine } else { PrintItem::SpaceOrNewLine });
            }

            items.push(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node_with_inner_parse(param, context, move |mut items| {
                if i < params_count - 1 {
                    items.push(",".into());
                }

                items
            }))).into());
        }

        items
    }

    fn get_use_new_lines(params: &Vec<Node>, context: &mut Context) -> bool {
        if params.is_empty() {
            false
        } else {
            let first_param = &params[0];
            let angle_bracket_token = context.get_first_angle_bracket_token_before(first_param);
            if let Some(angle_bracket_token) = angle_bracket_token {
                node_helpers::get_use_new_lines_for_nodes(&angle_bracket_token, first_param, context)
            } else {
                false
            }
        }
    }
}

/* comments */

fn parse_leading_comments(node: &dyn Spanned, context: &mut Context) -> Vec<PrintItem> {
    let leading_comments = node.leading_comments(context);
    parse_comments_as_leading(node, leading_comments, context)
}

fn parse_comments_as_leading(node: &dyn Spanned, comments: Vec<Comment>, context: &mut Context) -> Vec<PrintItem> {
    if comments.is_empty() {
        return vec![];
    }

    let last_comment = comments.last().unwrap().clone();
    let last_comment_previously_handled = context.has_handled_comment(&last_comment);
    let mut items = Vec::new();

    items.extend(parse_comment_collection(comments, None, context));

    if !last_comment_previously_handled {
        let node_start_line = node.start_line(context);
        let last_comment_end_line = last_comment.end_line(context);
        if node_start_line > last_comment_end_line {
            items.push(PrintItem::NewLine);

            if node_start_line - 1 > last_comment_end_line {
                items.push(PrintItem::NewLine);
            }
        }
        else if last_comment.kind == CommentKind::Block && node_start_line == last_comment_end_line {
            items.push(" ".into());
        }
    }

    items
}

fn parse_trailing_comments_as_statements(node: &dyn Spanned, context: &mut Context) -> Vec<PrintItem> {
    let unhandled_comments = get_trailing_comments_as_statements(node, context);
    parse_comment_collection(unhandled_comments, Some(node), context)
}

fn get_trailing_comments_as_statements(node: &dyn Spanned, context: &mut Context) -> Vec<Comment> {
    let mut items = Vec::new();
    let node_end_line = node.end_line(context);
    for comment in node.trailing_comments(context) {
        if !context.has_handled_comment(&comment) && node_end_line < comment.end_line(context) {
            items.push(comment);
        }
    }
    items
}

fn parse_comment_collection(comments: Vec<Comment>, last_node: Option<&dyn Spanned>, context: &mut Context) -> Vec<PrintItem> {
    let mut last_node = last_node;
    let mut items = Vec::new();
    for comment in comments.iter() {
        if !context.has_handled_comment(comment) {
            items.extend(parse_comment_based_on_last_node(comment, &last_node, context));
            last_node = Some(comment);
        }
    }
    items
}

fn parse_comment_based_on_last_node(comment: &Comment, last_node: &Option<&dyn Spanned>, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    if let Some(last_node) = last_node {
        if comment.start_line(context) > last_node.end_line(context) {
            items.push(PrintItem::NewLine);

            if comment.start_line(context) > last_node.end_line(context) + 1 {
                items.push(PrintItem::NewLine);
            }
        } else if comment.kind == CommentKind::Line || last_node.text(context).starts_with("/*") {
            items.push(" ".into());
        }
    }

    items.extend(parse_comment(&comment, context));
    return items;
}

fn parse_comment(comment: &Comment, context: &mut Context) -> Vec<PrintItem> {
    // only parse if handled
    if context.has_handled_comment(comment) {
        return Vec::new();
    }

    // mark handled and parse
    context.mark_comment_handled(comment);
    return match comment.kind {
        CommentKind::Block => parse_comment_block(comment),
        CommentKind::Line => parse_comment_line(comment),
    };

    fn parse_comment_block(comment: &Comment) -> Vec<PrintItem> {
        let mut vec = Vec::new();
        vec.push("/*".into());
        vec.extend(parse_raw_string(&comment.text));
        vec.push("*/".into());
        vec
    }

    fn parse_comment_line(comment: &Comment) -> Vec<PrintItem> {
        return vec![
            get_comment_text(&comment.text).into(),
            PrintItem::ExpectNewLine
        ];

        fn get_comment_text(original_text: &String) -> String {
            let non_slash_index = get_first_non_slash_index(&original_text);
            let start_text_index = if original_text.chars().skip(non_slash_index).next() == Some(' ') { non_slash_index + 1 } else { non_slash_index };
            let comment_text_original = original_text.chars().skip(start_text_index).collect::<String>();
            let comment_text = comment_text_original.trim_end();
            let prefix = format!("//{}", original_text.chars().take(non_slash_index).collect::<String>());

            return if comment_text.is_empty() {
                prefix
            } else {
                format!("{} {}", prefix, comment_text)
            };

            fn get_first_non_slash_index(text: &String) -> usize {
                let mut i: usize = 0;
                for c in text.chars() {
                    if c != '/' {
                        return i;
                    }
                    i += 1;
                }

                return i;
            }
        }
    }
}

fn parse_first_line_trailing_comments(node: &dyn Spanned, first_member: Option<&dyn Spanned>, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let node_start_line = node.start_line(context);

    for comment in get_comments(&node, &first_member, context) {
        if context.has_handled_comment(&comment) {
            continue;
        }

        if comment.start_line(context) == node_start_line {
            if comment.kind == CommentKind::Line {
                items.push(" ".into());
            }
            items.extend(parse_comment(&comment, context));
        }
    }

    return items;

    fn get_comments(node: &dyn Spanned, first_member: &Option<&dyn Spanned>, context: &mut Context) -> Vec<Comment> {
        let mut comments = Vec::new();
        // todo: inner comments?
        if let Some(first_member) = first_member {
            comments.extend(first_member.leading_comments(context));
        }
        comments.extend(node.trailing_comments(context));
        return comments;
    }
}

fn parse_trailing_comments(node: &dyn Spanned, context: &mut Context) -> Vec<PrintItem> {
    // todo: handle comments for object expr, arrayexpr, and tstupletype?
    let trailing_comments = node.trailing_comments(context);
    parse_comments_as_trailing(node, trailing_comments, context)
}

fn parse_comments_as_trailing(node: &dyn Spanned, trailing_comments: Vec<Comment>, context: &mut Context) -> Vec<PrintItem> {
    // use the roslyn definition of trailing comments
    let node_end_line = node.end_line(context);
    let trailing_comments_on_same_line = trailing_comments.into_iter().filter(|c| c.start_line(context) == node_end_line).collect::<Vec<Comment>>();
    let first_unhandled_comment = trailing_comments_on_same_line.iter().filter(|c| !context.has_handled_comment(&c)).next();
    let mut items = Vec::new();

    if let Some(first_unhandled_comment) = first_unhandled_comment {
        if first_unhandled_comment.kind == CommentKind::Block {
            items.push(" ".into());
        }
    }

    items.extend(parse_comment_collection(trailing_comments_on_same_line, Some(node), context));

    return items;
}

/* helpers */

struct ParseArrayLikeNodesOptions {
    node: Node,
    elements: Vec<Option<Node>>,
    trailing_commas: TrailingCommas,
}

fn parse_array_like_nodes(opts: ParseArrayLikeNodesOptions, context: &mut Context) -> Vec<PrintItem> {
    let node = opts.node;
    let elements = opts.elements;
    let use_new_lines = get_use_new_lines(&node, &elements, context);
    let force_trailing_commas = get_force_trailing_commas(&opts.trailing_commas, use_new_lines);
    let mut items = Vec::new();

    items.push("[".into());
    if !elements.is_empty() {
        items.extend(parse_elements(elements, use_new_lines, force_trailing_commas, context));
    }
    items.push("]".into());

    return items;

    fn parse_elements(elements: Vec<Option<Node>>, use_new_lines: bool, force_trailing_commas: bool, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();
        let elements_len = elements.len();

        if use_new_lines { items.push(PrintItem::NewLine); }

        for (i, element) in elements.into_iter().enumerate() {
            if i > 0 && !use_new_lines {
                items.push(PrintItem::SpaceOrNewLine);
            }

            let has_comma = force_trailing_commas || i < elements_len - 1;
            items.push(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_element(element, has_comma, context))).into());

            if use_new_lines { items.push(PrintItem::NewLine); }
        }

        return items;

        fn parse_element(element: Option<Node>, has_comma: bool, context: &mut Context) -> Vec<PrintItem> {
            if let Some(element) = element {
                parse_node_with_inner_parse(element, context, move |mut items| {
                    if has_comma { items.push(",".into()); }

                    items
                })
            } else {
                if has_comma { vec![",".into()] } else { vec![] }
            }
        }
    }

    fn get_use_new_lines(node: &Node, elements: &Vec<Option<Node>>, context: &mut Context) -> bool {
        if elements.is_empty() {
            false
        } else {
            let open_bracket_token = context.get_first_open_bracket_token_within(node).expect("Expected to find an open bracket token.");
            if let Some(first_node) = &elements[0] {
                node_helpers::get_use_new_lines_for_nodes(&open_bracket_token, first_node, context)
            } else {
                // todo: tests for this (ex. [\n,] -> [\n    ,\n])
                let first_comma = context.get_first_comma_within(&node);
                if let Some(first_comma) = first_comma {
                    node_helpers::get_use_new_lines_for_nodes(&open_bracket_token, &first_comma, context)
                } else {
                    false
                }
            }
        }
    }
}

struct ParseMemberedBodyOptions {
    span: Span,
    members: Vec<Node>,
    start_header_info: Option<Info>,
    brace_position: BracePosition,
    should_use_blank_line: Box<dyn Fn(&Node, &Node, &mut Context) -> bool>,
    trailing_commas: Option<TrailingCommas>
}

fn parse_membered_body(opts: ParseMemberedBodyOptions, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let open_brace_token = context.get_first_open_brace_token_before(&if opts.members.is_empty() { opts.span.hi() } else { opts.members[0].lo() });
    let close_brace_token = context.get_first_open_brace_token_before(&opts.span.hi());

    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: opts.brace_position,
        open_brace_token: &open_brace_token,
        start_header_info: opts.start_header_info,
    }, context));

    items.push("{".into());
    // items.extend(parse_first_line_trailing_comments()); // todo
    items.extend(parser_helpers::with_indent({
        let mut items = Vec::new();
        if !opts.members.is_empty() {
            items.push(PrintItem::NewLine);
        }

        items.extend(parse_statements_or_members(ParseStatementsOrMembersOptions {
            inner_span: Span::new(open_brace_token.hi(), close_brace_token.lo(), Default::default()),
            items: opts.members.into_iter().map(|node| (node, None)).collect(),
            should_use_space: None,
            should_use_new_line: None,
            should_use_blank_line: opts.should_use_blank_line,
            trailing_commas: opts.trailing_commas,
        }, context));

        items
    }));
    items.push(PrintItem::NewLine);
    items.push("}".into());

    items
}

fn parse_statements(inner_span: Span, stmts: Vec<Node>, context: &mut Context) -> Vec<PrintItem> {
    parse_statements_or_members(ParseStatementsOrMembersOptions {
        inner_span,
        items: stmts.into_iter().map(|stmt| (stmt, None)).collect(),
        should_use_space: None,
        should_use_new_line: None,
        should_use_blank_line: Box::new(|previous, next, context| node_helpers::has_separating_blank_line(previous, next, context)),
        trailing_commas: None,
    }, context)
}

struct ParseStatementsOrMembersOptions {
    inner_span: Span,
    items: Vec<(Node, Option<Vec<PrintItem>>)>,
    should_use_space: Option<Box<dyn Fn(&Node, &Node, &mut Context) -> bool>>,
    should_use_new_line: Option<Box<dyn Fn(&Node, &Node, &mut Context) -> bool>>,
    should_use_blank_line: Box<dyn Fn(&Node, &Node, &mut Context) -> bool>,
    trailing_commas: Option<TrailingCommas>,
}

fn parse_statements_or_members(opts: ParseStatementsOrMembersOptions, context: &mut Context) -> Vec<PrintItem> {
    let mut last_node: Option<Node> = None;
    let mut items = Vec::new();
    let children_len = opts.items.len();

    for (i, (node, optional_print_items)) in opts.items.into_iter().enumerate() {
        if let Some(last_node) = last_node {
            if should_use_new_line(&opts.should_use_new_line, &last_node, &node, context) {
                items.push(PrintItem::NewLine);

                if (opts.should_use_blank_line)(&last_node, &node, context) {
                    items.push(PrintItem::NewLine);
                }
            }
            else if let Some(should_use_space) = &opts.should_use_space {
                if should_use_space(&last_node, &node, context) {
                    items.push(PrintItem::SpaceOrNewLine);
                }
            }
        }

        let end_info = Info::new("endStatementOrMemberInfo");
        context.end_statement_or_member_infos.push(end_info.clone());
        items.extend(if let Some(print_items) = optional_print_items {
            print_items
        } else {
            let trailing_commas = opts.trailing_commas.clone();
            parse_node_with_inner_parse(node.clone(), context, move |mut items| {
                if let Some(trailing_commas) = &trailing_commas {
                    let force_trailing_commas = get_force_trailing_commas(trailing_commas, true);
                    if force_trailing_commas || i < children_len - 1 {
                        items.push(",".into())
                    }
                }
                items
            })
        });
        items.push(end_info.into());
        context.end_statement_or_member_infos.pop();

        last_node = Some(node);
    }

    if let Some(last_node) = &last_node {
        items.extend(parse_trailing_comments_as_statements(last_node, context));
    }

    if children_len == 0 {
        items.extend(parse_comment_collection(opts.inner_span.lo().trailing_comments(context), None, context));
    }

    return items;

    fn should_use_new_line(
        should_use_new_line: &Option<Box<dyn Fn(&Node, &Node, &mut Context) -> bool>>,
        last_node: &Node,
        next_node: &Node,
        context: &mut Context
    ) -> bool {
        if let Some(should_use) = &should_use_new_line {
            return (should_use)(last_node, next_node, context);
        }
        return true;
    }
}

struct ParseParametersOrArgumentsOptions {
    nodes: Vec<Node>,
    force_multi_line_when_multiple_lines: bool,
    custom_close_paren: Option<Vec<PrintItem>>,
}

fn parse_parameters_or_arguments(opts: ParseParametersOrArgumentsOptions, context: &mut Context) -> Vec<PrintItem> {
    let nodes = opts.nodes;
    let start_info = Info::new("startParamsOrArgs");
    let end_info = Info::new("endParamsOrArgs");
    let use_new_lines = get_use_new_lines(&nodes, context);
    let force_multi_lines_when_multiple_lines = opts.force_multi_line_when_multiple_lines;
    let is_single_function = nodes.len() == 1 && (match nodes[0].kind() { NodeKind::FnExpr | NodeKind::ArrowExpr => true, _ => false });
    let is_multi_line_or_hanging = {
        let start_info = start_info.clone(); // create copies
        let end_info = end_info.clone();
        move |condition_context: &mut ConditionResolverContext| {
            if use_new_lines { return Some(true); }
            if force_multi_lines_when_multiple_lines && !is_single_function {
                return condition_resolvers::is_multiple_lines(condition_context, &start_info, &end_info);
            }
            return Some(false);
        }
    };

    let mut items: Vec<PrintItem> = Vec::new();
    items.push(start_info.into());
    items.push("(".into());

    let param_list = parse_comma_separated_values(nodes, is_multi_line_or_hanging.clone(), context);
    items.push(Condition::new("multiLineOrHanging", ConditionProperties {
        condition: Box::new(is_multi_line_or_hanging),
        true_path: surround_with_new_lines(with_indent(param_list.clone())).into(),
        false_path: param_list.into(),
    }).into());

    if let Some(custom_close_paren) = opts.custom_close_paren {
        items.extend(custom_close_paren);
    }
    else {
        items.push(")".into());
    }

    items.push(end_info.into());

    return items;

    fn get_use_new_lines(nodes: &Vec<Node>, context: &mut Context) -> bool {
        if nodes.is_empty() {
            return false;
        }

        let first_node = &nodes[0];
        let open_paren_token = context.get_first_open_paren_token_before(first_node);

        if let Some(open_paren_token) = open_paren_token {
            node_helpers::get_use_new_lines_for_nodes(&open_paren_token, first_node, context)
        } else {
            false
        }
    }
}

struct ParseCloseParenWithTypeOptions {
    start_info: Info,
    type_node: Option<Node>,
    type_node_separator: Option<Vec<PrintItem>>,
}

fn parse_close_paren_with_type(opts: ParseCloseParenWithTypeOptions, context: &mut Context) -> Vec<PrintItem> {
    let type_node_start_info = Info::new("typeNodeStart");
    let type_node_end_info = Info::new("typeNodeEnd");
    let start_info = opts.start_info;
    let parsed_type_node = parse_type_node(opts.type_node, opts.type_node_separator, type_node_start_info.clone(), type_node_end_info.clone(), context);
    let mut items = Vec::new();

    items.push(Condition::new("newLineIfHeaderHangingAndTypeNodeMultipleLines", ConditionProperties {
        condition: Box::new(move |context| {
            if let Some(is_hanging) = condition_resolvers::is_hanging(context, &start_info, &None) {
                if let Some(is_multiple_lines) = condition_resolvers::is_multiple_lines(context, &type_node_start_info, &type_node_end_info) {
                    return Some(is_hanging && is_multiple_lines);
                }
            }
            return None;
        }),
        true_path: Some(vec![PrintItem::NewLine]),
        false_path: None,
    }).into());
    items.push(")".into());
    items.extend(parsed_type_node);
    return items;

    fn parse_type_node(
        type_node: Option<Node>,
        type_node_separator: Option<Vec<PrintItem>>,
        type_node_start_info: Info,
        type_node_end_info: Info,
        context: &mut Context
    ) -> Vec<PrintItem> {
        let mut items = Vec::new();
        if let Some(type_node) = type_node {
            items.push(type_node_start_info.into());
            if let Some(type_node_separator) = type_node_separator {
                items.extend(type_node_separator);
            } else {
                if context.config.type_annotation_space_before_colon { items.push(" ".into()); }
                items.push(": ".into());
            }
            items.extend(parse_node(type_node.into(), context));
            items.push(type_node_end_info.into());
        }
        return items;
    }
}

fn parse_comma_separated_values(
    values: Vec<Node>,
    multi_line_or_hanging_condition_resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    context: &mut Context
) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let values_count = values.len();

    for (i, value) in values.into_iter().enumerate() {
        let has_comma = i < values_count - 1;
        let parsed_value = parse_value(value, has_comma, context);

        if i == 0 {
            items.extend(parsed_value);
        } else {
            items.push(Condition::new("multiLineOrHangingCondition", ConditionProperties {
                condition: Box::new(multi_line_or_hanging_condition_resolver.clone()),
                true_path: {
                    let mut items = Vec::new();
                    items.push(PrintItem::NewLine);
                    items.extend(parsed_value.clone());
                    Some(items)
                },
                false_path: {
                    let mut items = Vec::new();
                    items.push(PrintItem::SpaceOrNewLine);
                    items.push(conditions::indent_if_start_of_line(parsed_value).into());
                    Some(items)
                },
            }).into());
        }
    }

    return items;

    fn parse_value(value: Node, has_comma: bool, context: &mut Context) -> Vec<PrintItem> {
        parser_helpers::new_line_group(parse_node_with_inner_parse(value, context, move |mut items| {
            if has_comma {
                items.push(",".into());
            }
            items
        }))
    }
}

fn parse_type_annotation_with_colon_if_exists(type_ann: Option<TsTypeAnn>, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if let Some(type_ann) = type_ann {
        if context.config.type_annotation_space_before_colon {
            items.push(" ".into());
        }
        items.extend(parse_node_with_preceeding_colon(Some(type_ann.into()), context));
    }
    items
}

fn parse_node_with_preceeding_colon(node: Option<Node>, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if let Some(node) = node {
        items.push(":".into());
        items.push(PrintItem::SpaceOrNewLine);
        items.push(conditions::indent_if_start_of_line(parse_node(node, context)).into());
    }
    items
}

struct ParseBraceSeparatorOptions<'a> {
    brace_position: BracePosition,
    open_brace_token: &'a Option<TokenAndSpan>,
    start_header_info: Option<Info>,
}

fn parse_brace_separator<'a>(opts: ParseBraceSeparatorOptions<'a>, context: &mut Context) -> Vec<PrintItem> {
    match opts.brace_position {
        BracePosition::NextLineIfHanging => {
            if let Some(start_header_info) = opts.start_header_info {
                vec![conditions::new_line_if_hanging_space_otherwise(conditions::NewLineIfHangingSpaceOtherwiseOptions {
                    start_info: start_header_info,
                    end_info: None,
                    space_char: None,
                }).into()]
            } else {
                vec![" ".into()]
            }
        },
        BracePosition::SameLine => {
            vec![" ".into()]
        },
        BracePosition::NextLine => {
            vec![PrintItem::NewLine]
        },
        BracePosition::Maintain => {
            vec![if let Some(open_brace_token) = opts.open_brace_token {
                if node_helpers::is_first_node_on_line(open_brace_token, context) {
                    PrintItem::NewLine
                } else {
                    " ".into()
                }
            } else {
                " ".into()
            }]
        },
    }
}

fn parse_node_in_parens(first_inner_node: &dyn Ranged, inner_parsed_node: Vec<PrintItem>, context: &mut Context) -> Vec<PrintItem> {
    let open_paren_token = context.get_first_open_paren_token_before(&first_inner_node);
    let use_new_lines = {
        if let Some(open_paren_token) = &open_paren_token {
            node_helpers::get_use_new_lines_for_nodes(open_paren_token, &first_inner_node, context)
        } else {
            false
        }
    };

    // todo: disable indent?

    return wrap_in_parens(inner_parsed_node, use_new_lines, context);
}

fn wrap_in_parens(parsed_node: Vec<PrintItem>, use_new_lines: bool, context: &mut Context) -> Vec<PrintItem> {
    parser_helpers::new_line_group({
        let mut items = Vec::new();
        items.push("(".into());
        if use_new_lines {
            items.push(PrintItem::NewLine);
            items.extend(parser_helpers::with_indent(parsed_node));
            items.push(PrintItem::NewLine);
        } else {
            items.extend(parsed_node);
        }
        items.push(")".into());
        items
    })
}

fn parse_extends_or_implements(text: &str, type_items: Vec<Node>, start_header_info: Info, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    if type_items.is_empty() {
        return items;
    }

    items.push(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_header_info, None).into());
    // the newline group will force it to put the extends or implements on a new line
    items.push(conditions::indent_if_start_of_line(parser_helpers::new_line_group({
        let mut items = Vec::new();
        items.push(format!("{} ", text).into());
        for (i, type_item) in type_items.into_iter().enumerate() {
            if i > 0 {
                items.push(",".into());
                items.push(PrintItem::SpaceOrNewLine);
            }

            items.push(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node(type_item, context))).into());
        }
        items
    })).into());

    return items;
}

struct ParseObjectLikeNodeOptions {
    node_span: Span,
    members: Vec<Node>,
    trailing_commas: Option<TrailingCommas>,
}

fn parse_object_like_node(opts: ParseObjectLikeNodeOptions, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    if opts.members.is_empty() {
        items.push("{}".into()); // todo: comments?
        return items;
    }

    let open_brace_token = context.get_first_open_brace_token_within(&opts.node_span).expect("Expected to find an open brace token.");
    let close_brace_token = context.get_first_close_brace_token_before(&opts.node_span.hi()).expect("Expected to find a close brace token.");
    let multi_line = node_helpers::get_use_new_lines_for_nodes(
        &open_brace_token,
        &opts.members[0],
        context
    );
    let start_info = Info::new("startObject");
    let end_info = Info::new("startObject");
    let separator = if multi_line { PrintItem::NewLine } else { " ".into() };

    items.push(start_info.clone().into());
    items.push("{".into());
    items.push(separator.clone().into());

    if multi_line {
        items.extend(parser_helpers::with_indent(parse_statements_or_members(ParseStatementsOrMembersOptions {
            inner_span: Span::new(open_brace_token.hi(), close_brace_token.lo(), Default::default()),
            items: opts.members.into_iter().map(|member| (member.into(), None)).collect(),
            should_use_space: None,
            should_use_new_line: None,
            should_use_blank_line: Box::new(|previous, next, context| node_helpers::has_separating_blank_line(previous, next, context)),
            trailing_commas: opts.trailing_commas,
        }, context)));
    } else {
        let members_len = opts.members.len();
        for (i, member) in opts.members.into_iter().enumerate() {
            if i > 0 { items.push(PrintItem::SpaceOrNewLine); }

            let trailing_commas = opts.trailing_commas.clone();
            items.push(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node_with_inner_parse(member, context, move |mut items| {
                if let Some(trailing_commas) = &trailing_commas {
                    if i < members_len - 1 || get_force_trailing_commas(trailing_commas, multi_line) {
                        items.push(",".into());
                    }
                }
                items
            }))).into());
        }
    }

    items.push(separator.into());
    items.push("}".into());
    items.push(end_info.clone().into());

    return items;
}

struct MemberLikeExpr {
    left_node: Node,
    right_node: Node,
    is_computed: bool,
}

fn parse_for_member_like_expr(node: MemberLikeExpr, context: &mut Context) -> Vec<PrintItem> {
    let use_new_line = node_helpers::get_use_new_lines_for_nodes(&node.left_node, &node.right_node, context);
    let mut items = Vec::new();
    let is_optional = context.parent().kind() == NodeKind::OptChainExpr;

    items.extend(parse_node(node.left_node, context));
    items.push(if use_new_line { PrintItem::NewLine } else { PrintItem::PossibleNewLine });
    items.push(conditions::indent_if_start_of_line({
        let mut items = Vec::new();

        if is_optional {
            items.push("?".into());
            if node.is_computed { items.push(".".into()); }
        }
        items.push(if node.is_computed { "[" } else { "." }.into());
        items.extend(parse_node(node.right_node, context));
        if node.is_computed { items.push("]".into()); }

        items
    }).into());

    return items;
}

fn parse_decorators(decorators: Vec<Decorator>, is_class_expression: bool, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    if decorators.is_empty() {
        return items;
    }

    let use_new_lines = !is_class_expression
        && decorators.len() >= 2
        && node_helpers::get_use_new_lines_for_nodes(&decorators[0], &decorators[1], context);

    for (i, decorator) in decorators.into_iter().enumerate() {
        if i > 0 {
            items.push(if use_new_lines {
                PrintItem::NewLine
            } else {
                PrintItem::SpaceOrNewLine
            });
        }

        let parsed_node = parse_node(decorator.into(), context);
        if is_class_expression {
            items.push(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parsed_node)).into());
        } else {
            items.extend(parser_helpers::new_line_group(parsed_node));
        }
    }

    items.push(if is_class_expression {
        PrintItem::SpaceOrNewLine
    } else {
        PrintItem::NewLine
    });

    return items;
}

fn parse_control_flow_separator(
    next_control_flow_position: NextControlFlowPosition,
    node_block: &Span,
    token_text: &str,
    context: &mut Context
) -> Vec<PrintItem> {
    let mut items = Vec::new();
    match next_control_flow_position {
        NextControlFlowPosition::SameLine => items.push(" ".into()),
        NextControlFlowPosition::NextLine => items.push(PrintItem::NewLine),
        NextControlFlowPosition::Maintain => {
            let token = if token_text == "catch" {
                context.get_first_token_within_with_text(node_block, token_text)
            } else {
                context.get_first_token_before_with_text(node_block, token_text)
            };

            if token.is_some() && node_helpers::is_first_node_on_line(&token.unwrap(), context) {
                items.push(PrintItem::NewLine);
            } else {
                items.push(" ".into());
            }
        }
    }
    return items;
}

struct ParseHeaderWithConditionalBraceBodyOptions<'a> {
    parent: &'a Span,
    body_node: Node,
    parsed_header: Vec<PrintItem>,
    use_braces: UseBraces,
    brace_position: BracePosition,
    single_body_position: Option<SingleBodyPosition>,
    requires_braces_condition: Option<Condition>,
}

struct ParseHeaderWithConditionalBraceBodyResult {
    parsed_node: Vec<PrintItem>,
    open_brace_condition: Condition,
}

fn parse_header_with_conditional_brace_body<'a>(opts: ParseHeaderWithConditionalBraceBodyOptions<'a>, context: &mut Context) -> ParseHeaderWithConditionalBraceBodyResult {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = Vec::new();

    items.push(start_header_info.clone().into());
    items.extend(opts.parsed_header);
    items.push(end_header_info.clone().into());
    let result = parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: opts.parent,
        body_node: opts.body_node,
        use_braces: opts.use_braces,
        brace_position: opts.brace_position,
        single_body_position: opts.single_body_position,
        requires_braces_condition: opts.requires_braces_condition,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context);
    items.extend(result.parsed_node);

    return ParseHeaderWithConditionalBraceBodyResult {
        open_brace_condition: result.open_brace_condition,
        parsed_node: items,
    };
}

struct ParseConditionalBraceBodyOptions<'a> {
    parent: &'a Span,
    body_node: Node,
    use_braces: UseBraces,
    brace_position: BracePosition,
    single_body_position: Option<SingleBodyPosition>,
    requires_braces_condition: Option<Condition>,
    header_start_token: Option<TokenAndSpan>,
    start_header_info: Option<Info>,
    end_header_info: Option<Info>,
}

struct ParseConditionalBraceBodyResult {
    parsed_node: Vec<PrintItem>,
    open_brace_condition: Condition,
}

fn parse_conditional_brace_body<'a>(opts: ParseConditionalBraceBodyOptions<'a>, context: &mut Context) -> ParseConditionalBraceBodyResult {
    let start_header_info = opts.start_header_info;
    let end_header_info = opts.end_header_info;
    let requires_braces_condition = opts.requires_braces_condition;
    let start_statements_info = Info::new("startStatements");
    let end_statements_info = Info::new("endStatements");
    let header_trailing_comments = get_header_trailing_comments(&opts.body_node, context);
    let body_should_be_multi_line = get_body_should_be_multi_line(&opts.body_node, &header_trailing_comments, context);
    let should_use_new_line = get_should_use_new_line(
        &opts.body_node,
        body_should_be_multi_line,
        &opts.single_body_position,
        &opts.header_start_token,
        &opts.parent,
        context
    );
    let open_brace_token = get_open_brace_token(&opts.body_node, context);
    let use_braces = opts.use_braces;
    let newline_or_space_condition = Condition::new("newLineOrSpace", ConditionProperties {
        condition: {
            let start_header_info = start_header_info.clone();
            let end_statements_info = end_statements_info.clone();
            Box::new(move |condition_context| {
                if should_use_new_line {
                    return Some(true);
                }
                let start_header_info = start_header_info.as_ref()?;
                let resolved_start_info = condition_context.get_resolved_info(start_header_info)?;
                if resolved_start_info.line_number < condition_context.writer_info.line_number {
                    return Some(true);
                }
                let resolved_end_statements_info = condition_context.get_resolved_info(&end_statements_info)?;
                return Some(resolved_end_statements_info.line_number > resolved_start_info.line_number);
            })
        },
        true_path: Some(vec![PrintItem::NewLine]),
        false_path: Some(vec![" ".into()]),
    });
    let open_brace_condition = Condition::new("openBrace", ConditionProperties {
        condition: {
            let start_header_info = start_header_info.clone();
            let end_header_info = end_header_info.clone();
            let start_statements_info = start_statements_info.clone();
            let end_statements_info = end_statements_info.clone();
            let open_brace_token = open_brace_token.clone();
            let newline_or_space_condition = newline_or_space_condition.clone();
            Box::new(move |condition_context| {
                match use_braces {
                    UseBraces::WhenNotSingleLine => condition_context.get_resolved_condition(&newline_or_space_condition),
                    UseBraces::Maintain => Some(open_brace_token.is_some()),
                    UseBraces::Always => Some(true),
                    UseBraces::PreferNone => {
                        // writing an open brace might make the header hang, so assume it should
                        // not write the open brace until it's been resolved
                        if body_should_be_multi_line {
                            return Some(true);
                        }
                        if let Some(start_header_info) = &start_header_info {
                            if let Some(end_header_info) = &end_header_info {
                                let is_multiple_lines = condition_resolvers::is_multiple_lines(condition_context, start_header_info, end_header_info)?;
                                if is_multiple_lines {
                                    return Some(true);
                                }
                            }
                        }
                        let is_multiple_lines = condition_resolvers::is_multiple_lines(condition_context, &start_statements_info, &end_statements_info)?;
                        if is_multiple_lines {
                            return Some(true);
                        }

                        if let Some(requires_braces_condition) = &requires_braces_condition {
                            let requires_braces = condition_context.get_resolved_condition(requires_braces_condition)?;
                            if requires_braces {
                                return Some(true);
                            }
                        }
                        return Some(false);
                    }
                }
            })
        },
        true_path: {
            let mut items = Vec::new();
            items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
                brace_position: opts.brace_position,
                open_brace_token: &open_brace_token,
                start_header_info: start_header_info.clone(),
            }, context));
            items.push("{".into());
            Some(items)
        },
        false_path: None,
    });

    // parse body
    let mut items = Vec::new();
    items.push(open_brace_condition.clone().into());
    items.extend(parser_helpers::prepend_if_has_items(parse_comment_collection(header_trailing_comments, None, context), " ".into()));
    items.push(newline_or_space_condition.clone().into());
    items.push(start_statements_info.clone().into());

    if let Node::BlockStmt(body_node) = opts.body_node {
        items.extend(parser_helpers::with_indent({
            let mut items = Vec::new();
            // parse the remaining trailing comments inside because some of them are parsed already
            // by parsing the header trailing comments
            items.extend(parse_leading_comments(&body_node, context));
            items.extend(parse_statements(body_node.get_inner_span(context), body_node.stmts.into_iter().map(|x| x.into()).collect(), context));
            items
        }));
    } else {
        items.extend(parser_helpers::with_indent({
            let mut items = Vec::new();
            let body_node_span = opts.body_node.span();
            items.extend(parse_node(opts.body_node, context));
            items.extend(parse_trailing_comments(&body_node_span, context));
            items
        }));
    }

    items.push(end_statements_info.clone().into());
    items.push(Condition::new("closeBrace", ConditionProperties {
        condition: {
            let open_brace_condition = open_brace_condition.clone();
            Box::new(move |condition_context| condition_context.get_resolved_condition(&open_brace_condition))
        },
        true_path: Some(vec![
            Condition::new("closeBraceNewLine", ConditionProperties {
                condition: {
                    let newline_or_space_condition = newline_or_space_condition.clone();
                    Box::new(move |condition_context| {
                        let is_new_line = condition_context.get_resolved_condition(&newline_or_space_condition)?;
                        if !is_new_line { return Some(false); }
                        let are_infos_equal = condition_resolvers::are_infos_equal(condition_context, &start_statements_info, &end_statements_info)?;
                        return Some(!are_infos_equal);
                    })
                },
                true_path: Some(vec![PrintItem::NewLine]),
                false_path: Some(vec![Condition::new("closeBraceSpace", ConditionProperties {
                    condition: Box::new(move |condition_context| {
                        let is_new_line = condition_context.get_resolved_condition(&newline_or_space_condition)?;
                        return Some(!is_new_line);
                    }),
                    true_path: Some(vec![" ".into()]),
                    false_path: None,
                }).into()])
            }).into(),
            "}".into()
        ]),
        false_path: None,
    }).into());

    // return result
    return ParseConditionalBraceBodyResult {
        parsed_node: items,
        open_brace_condition,
    };

    fn get_should_use_new_line(
        body_node: &Node,
        body_should_be_multi_line: bool,
        single_body_position: &Option<SingleBodyPosition>,
        header_start_token: &Option<TokenAndSpan>,
        parent: &Span,
        context: &mut Context
    ) -> bool {
        if body_should_be_multi_line {
            return true;
        }
        if let Some(single_body_position) = single_body_position {
            return match single_body_position {
                SingleBodyPosition::Maintain => get_body_stmt_start_line(body_node, context) > get_header_start_line(header_start_token, parent, context),
                SingleBodyPosition::NextLine => true,
                SingleBodyPosition::SameLine => {
                    if let Node::BlockStmt(block_stmt) = body_node {
                        if block_stmt.stmts.len() != 1 {
                            return true;
                        }
                        return get_body_stmt_start_line(body_node, context) > get_header_start_line(header_start_token, parent, context);
                    }
                    return false;
                },
            }
        } else {
            return true;
        }

        fn get_body_stmt_start_line(body_node: &Node, context: &mut Context) -> usize {
            if let Node::BlockStmt(body_node) = body_node {
                if let Some(first_stmt) = body_node.stmts.get(0) {
                    return first_stmt.start_line(context);
                }
            }
            return body_node.start_line(context);
        }

        fn get_header_start_line(header_start_token: &Option<TokenAndSpan>, parent: &Span, context: &mut Context) -> usize {
            if let Some(header_start_token) = header_start_token {
                return header_start_token.start_line(context);
            }
            return parent.start_line(context);
        }
    }

    fn get_body_should_be_multi_line(body_node: &Node, header_trailing_comments: &Vec<Comment>, context: &mut Context) -> bool {
        let mut has_leading_comment_on_different_line = |node: &dyn Ranged| {
            node_helpers::has_leading_comment_on_different_line(
                node,
                /* comments to ignore */ Some(header_trailing_comments),
                context
            )
        };
        if let Node::BlockStmt(body_node) = body_node {
            if body_node.stmts.len() == 1 && !has_leading_comment_on_different_line(&body_node.stmts[0]) {
                return false;
            }
            return true;
        } else {
            return has_leading_comment_on_different_line(&body_node);
        }
    }

    fn get_header_trailing_comments(body_node: &Node, context: &mut Context) -> Vec<Comment> {
        let mut comments = Vec::new();
        if let Node::BlockStmt(block_stmt) = body_node {
            let open_brace_token = context.get_first_open_brace_token_within(&block_stmt);
            let comment_line = body_node.leading_comments(context).into_iter().filter(|c| c.kind == CommentKind::Line).next();
            if let Some(comment) = comment_line {
                comments.push(comment);
                return comments;
            }

            let body_node_start_line = body_node.start_line(context);
            comments.extend(open_brace_token.trailing_comments(context).into_iter().filter(|c| c.start_line(context) == body_node_start_line));
        } else {
            let leading_comments = body_node.leading_comments(context);
            let last_header_token = context.get_first_non_comment_token_before(body_node);
            if let Some(last_header_token) = last_header_token {
                let last_header_token_end_line = last_header_token.end_line(context);
                comments.extend(leading_comments.into_iter().filter(|c| c.start_line(context) <= last_header_token_end_line));
            }
        }

        return comments;
    }

    fn get_open_brace_token(body_node: &Node, context: &mut Context) -> Option<TokenAndSpan> {
        if let Node::BlockStmt(block_stmt) = body_node {
            context.get_first_open_brace_token_within(&block_stmt)
        } else {
            None
        }
    }
}

/* is functions */

fn is_expr_template(node: &Expr) -> bool { // todo: remove
    match node {
        Expr::Tpl(_) => true,
        _ => false
    }
}

/* config helpers */

fn get_force_trailing_commas(option: &TrailingCommas, use_new_lines: bool) -> bool {
    match option {
        TrailingCommas::Always => true,
        TrailingCommas::OnlyMultiLine => use_new_lines,
        TrailingCommas::Never => false,
    }
}

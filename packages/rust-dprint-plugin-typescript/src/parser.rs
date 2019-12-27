extern crate dprint_core;

use std::rc::Rc;

use dprint_core::*;
use dprint_core::{parser_helpers::*,condition_resolvers};
use super::*;
use super::configuration::{BracePosition, MemberSpacing, TrailingCommas};
use swc_ecma_ast::{CallExpr, Module, Expr, ExprStmt, BigInt, Bool, JSXText, Number, Regex, Str, ExprOrSuper, Ident, ExprOrSpread, BreakStmt,
    ContinueStmt, DebuggerStmt, EmptyStmt, TsExportAssignment, ArrayLit, ArrayPat, TsTypeAnn, VarDecl, VarDeclKind, VarDeclarator, ExportAll,
    TsEnumDecl, TsEnumMember, TsTypeAliasDecl, TsLitType, TsNamespaceExportDecl, ExportDecl, ExportDefaultDecl, NamedExport, ExportSpecifier,
    DefaultExportSpecifier, NamespaceExportSpecifier, NamedExportSpecifier, ExportDefaultExpr, ImportDecl, ImportDefault, ImportSpecific,
    ImportStarAs, ImportSpecifier, TsImportEqualsDecl};
use swc_common::{comments::{Comment, CommentKind}};

pub fn parse(source_file: ParsedSourceFile, config: TypeScriptConfiguration) -> Vec<PrintItem> {
    let mut context = Context::new(
        config,
        source_file.comments,
        source_file.tokens,
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
    // println!("Node kind: {:?}", node.kind());
    // println!("Text: {:?}", node.text(context));

    // store info
    let past_current_node = std::mem::replace(&mut context.current_node, node.clone());
    context.parent_stack.push(past_current_node);

    // comments

    // now parse items
    let items = inner_parse(parse_node(node, context));

    // pop info
    context.current_node = context.parent_stack.pop().unwrap();

    return items;

    fn parse_node(node: Node, context: &mut Context) -> Vec<PrintItem> {
        match node {
            /* common */
            Node::Ident(node) => parse_identifier(node, context),
            /* declarations */
            Node::ExportDecl(node) => parse_export_decl(node, context),
            Node::ExportDefaultDecl(node) => parse_export_default_decl(node, context),
            Node::ExportDefaultExpr(node) => parse_export_default_expr(node, context),
            Node::ImportDecl(node) => parse_import_decl(node, context),
            Node::NamedExport(node) => parse_export_named_decl(node, context),
            Node::TsEnumDecl(node) => parse_enum_decl(node, context),
            Node::TsEnumMember(node) => parse_enum_member(node, context),
            Node::TsImportEqualsDecl(node) => parse_import_equals_decl(node, context),
            Node::TsTypeAliasDecl(node) => parse_type_alias(node, context),
            /* expressions */
            Node::ArrayLit(node) => parse_array_expression(node, context),
            Node::CallExpr(node) => parse_call_expression(node, context),
            Node::ExprOrSpread(node) => parse_expr_or_spread(node, context),
            Node::FnExpr(node) => vec![node.text(context).into()], // todo
            Node::ArrowExpr(node) => vec![node.text(context).into()], // todo
            /* exports */
            Node::NamedExportSpecifier(node) => parse_export_named_specifier(node, context),
            /* imports */
            Node::ImportSpecific(node) => parse_import_named_specifier(node, context),
            Node::ImportStarAs(node) => parse_import_namespace_specifier(node, context),
            Node::ImportDefault(node) => parse_node(node.local.into(), context),
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
            /* statements */
            Node::BreakStmt(node) => parse_break_stmt(node, context),
            Node::ContinueStmt(node) => parse_continue_stmt(node, context),
            Node::DebuggerStmt(node) => parse_debugger_stmt(node, context),
            Node::ExportAll(node) => parse_export_all(node, context),
            Node::ExprStmt(node) => parse_expr_stmt(node, context),
            Node::EmptyStmt(node) => parse_empty_stmt(node, context),
            Node::TsExportAssignment(node) => parse_export_assignment(node, context),
            Node::TsNamespaceExportDecl(node) => parse_namespace_export(node, context),
            Node::VarDecl(node) => parse_var_decl(node, context),
            Node::VarDeclarator(node) => parse_var_declarator(node, context),
            /* types */
            Node::TsLitType(node) => parse_lit_type(node, context),
            Node::TsTypeAnn(node) => parse_type_ann(node, context),
            Node::TsTypeParamInstantiation(node) => parse_type_param_instantiation(TypeParamNode::Instantiation(node), context),
            Node::TsTypeParamDecl(node) => parse_type_param_instantiation(TypeParamNode::Decl(node), context),
            Node::TsTypeParam(node) => vec![node.text(context).into()], // todo
            /* unknown */
            Node::TokenAndSpan(span) => vec![context.get_text(&span.span.data()).into()],
            Node::Comment(comment) => vec![context.get_text(&comment.span.data()).into()],
            Node::Unknown(span) => vec![context.get_text(&span.data()).into()],
            _ => vec![node.text(context).into()]
        }
    }
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

fn parse_export_decl(node: ExportDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    // todo: parse decorators if class
    items.push("export ".into());
    items.extend(parse_node(node.decl.into(), context));
    items
}

fn parse_export_default_decl(node: ExportDefaultDecl, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    // todo: parse decorators if class
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
    let node_clone = node.clone();

    // header
    items.push(start_header_info.into_clone());

    if node.declare { items.push("declare ".into()); }
    if node.is_const { items.push("const ".into()); }
    items.push("enum ".into());
    items.extend(parse_node(node.id.into(), context));

    // body
    let member_spacing = context.config.enum_declaration_member_spacing.clone();
    items.extend(parse_membered_body(ParseMemberedBodyOptions {
        node: node_clone.into(),
        members: node.members.into_iter().map(|x| x.into()).collect(),
        start_header_info: Some(start_header_info),
        brace_position: context.config.enum_declaration_brace_position.clone(),
        should_use_blank_line: Box::new(move |previous, next, context| {
            match member_spacing {
                MemberSpacing::BlankLine => true,
                MemberSpacing::NewLine => false,
                MemberSpacing::Maintain => node_helpers::has_separating_blank_line(previous, next, context),
            }
        }),
        trailing_commas: Some(context.config.enum_declaration_trailing_commas.clone()),
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
    let mut default_export: Option<DefaultExportSpecifier> = Option::None;
    let mut namespace_export: Option<NamespaceExportSpecifier> = Option::None;
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

fn parse_import_decl(node: ImportDecl, context: &mut Context) -> Vec<PrintItem> {
    // todo: rewrite this so that it doesn't need to clone the current node

    // fill specifiers
    let mut default_import: Option<ImportDefault> = Option::None;
    let mut namespace_import: Option<ImportStarAs> = Option::None;
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

    items
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

    items
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

    items
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

fn parse_array_expression(node: ArrayLit, context: &mut Context) -> Vec<PrintItem> {
    parse_array_like_nodes(ParseArrayLikeNodesOptions {
        node: node.clone().into(),
        elements: node.elems.into_iter().map(|x| x.map(|elem| elem.into())).collect(),
        trailing_commas: context.config.array_expression_trialing_commas.clone(),
    }, context)
}

fn parse_call_expression(node: CallExpr, context: &mut Context) -> Vec<PrintItem> {
    return if is_test_library_call_expression(&node, context) {
        parse_test_library_call_expr(node, context)
    } else {
        inner_parse(node, context)
    };

    fn inner_parse(node: CallExpr, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();

        items.extend(parse_node(node.callee.clone().into(), context));

        if let Some(type_args) = node.type_args {
            items.extend(parse_node(Node::TsTypeParamInstantiation(type_args), context));
        }

        items.push(conditions::with_indent_if_start_of_line_indented(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
            nodes: node.args.into_iter().map(|node| node.into()).collect(),
            force_multi_line_when_multiple_lines: context.config.call_expression_force_multi_line_arguments,
            custom_close_paren: Option::None,
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
    fn is_test_library_call_expression(node: &CallExpr, context: &mut Context) -> bool {
        if node.args.len() != 2 || node.type_args.is_some() || !is_valid_callee(&node.callee) {
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
                    ExprOrSuper::Super(_) => Option::None,
                    ExprOrSuper::Expr(box expr) => {
                        match expr {
                            Expr::Ident(ident) => Some(&ident.sym),
                            Expr::Member(member) if (*member.prop).kind() == NodeKind::Ident => get_identifier_text(&member.obj),
                            _ => Option::None,
                        }
                    }
                };
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
    items
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

fn parse_reg_exp_literal(node: Regex, context: &mut Context) -> Vec<PrintItem> {
    // the exp and flags should not be nodes so just ignore that (swc issue #511)
    let mut items = Vec::new();
    items.push("/".into());
    items.push(String::from(&node.exp as &str).into());
    items.push("/".into());
    items.push(String::from(&node.flags as &str).into());
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
        // this is done because the string literal token has the wrong position, so get the token (temp workaround for swc bug)
        let token = context.get_token_at(node);
        let raw_string_text = token.text(context);
        let string_value = raw_string_text.chars().skip(1).take(raw_string_text.chars().count() - 2).collect::<String>();
        let is_double_quote = string_value.chars().next().unwrap() == '"';

        match is_double_quote {
            true => string_value.replace("\\\"", "\""),
            false => string_value.replace("\\'", "'"),
        }
    }
}

/* module */

fn parse_module(node: Module, context: &mut Context) -> Vec<PrintItem> {
    parse_statements_or_members(ParseStatementsOrMembersOptions {
        items: node.body.into_iter().map(|module_item| (module_item.into(), Option::None)).collect(),
        last_node: Option::None,
        should_use_space: Option::None,
        should_use_new_line: Option::None,
        should_use_blank_line: Box::new(|previous, next, context| node_helpers::has_separating_blank_line(previous, next, context)),
        trailing_commas: Option::None,
    }, context)
}

/* patterns */

fn parse_array_pat(node: ArrayPat, context: &mut Context) -> Vec<PrintItem> {
    let mut items = parse_array_like_nodes(ParseArrayLikeNodesOptions {
        node: node.clone().into(),
        elements: node.elems.into_iter().map(|x| x.map(|elem| elem.into())).collect(),
        trailing_commas: context.config.array_pattern_trialing_commas.clone(),
    }, context);
    items.extend(parse_type_annotation_with_colon_if_exists(node.type_ann, context));
    items
}

/* statements */

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

fn parse_debugger_stmt(node: DebuggerStmt, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    items.push("debugger".into());
    if context.config.debugger_statement_semi_colon {
        items.push(";".into());
    }

    items
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

fn parse_empty_stmt(node: EmptyStmt, context: &mut Context) -> Vec<PrintItem> {
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

            Option::None
        }
    }
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

    if requires_semi_colon(context) { items.push(";".into()); }

    return items;

    fn requires_semi_colon(context: &mut Context) -> bool {
        // let parent_kind = context.parent().kind();
        //if (context.parent.type === "ForOfStatement" || context.parent.type === "ForInStatement")
        //    return context.parent.left !== node;

        //return context.config["variableStatement.semiColon"] || context.parent.type === "ForStatement";
        context.config.variable_statement_semi_colon
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

/* types */

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

fn parse_leading_comments(node: &Node, context: &mut Context) -> Vec<PrintItem> {
    let leading_comments = node.leading_comments(context);
    parse_comments_as_leading(node, leading_comments, context)
}

fn parse_comments_as_leading(node: &Node, comments: Vec<Comment>, context: &mut Context) -> Vec<PrintItem> {
    if comments.is_empty() {
        return vec![];
    }

    let last_comment = comments.last().unwrap().clone();
    let last_comment_previously_handled = context.has_handled_comment(&last_comment);
    let mut items = Vec::new();

    items.extend(parse_comment_collection(comments, Option::None, context));

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

fn parse_trailing_comments_as_statements(node: Node, context: &mut Context) -> Vec<PrintItem> {
    let unhandled_comments = get_trailing_comments_as_statements(node.clone(), context);
    parse_comment_collection(unhandled_comments, Some(node), context)
}

fn get_trailing_comments_as_statements(node: Node, context: &mut Context) -> Vec<Comment> {
    let mut items = Vec::new();
    for comment in node.trailing_comments(context) {
        if context.has_handled_comment(&comment) && node.end_line(context) < comment.end_line(context) {
            items.push(comment);
        }
    }
    items
}

fn parse_comment_collection(comments: Vec<Comment>, last_node: Option<Node>, context: &mut Context) -> Vec<PrintItem> {
    let mut last_node = last_node;
    let mut items = Vec::new();
    for comment in comments {
        if !context.has_handled_comment(&comment) {
            items.extend(parse_comment_based_on_last_node(&comment, &last_node, context));
            last_node = Some(comment.into());
        }
    }
    items
}

fn parse_comment_based_on_last_node(comment: &Comment, last_node: &Option<Node>, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    if let Some(last_node) = last_node {
        if comment.start_line(context) > last_node.end_line(context) {
            items.push(PrintItem::NewLine);

            if comment.start_line(context) > last_node.end_line(context) + 1 {
                items.push(PrintItem::NewLine);
            }
        } else if comment.kind == CommentKind::Line {
            items.push(" ".into());
        } //else let // todo: last_node is comment block
    }

    items.extend(parse_comment(&comment, context));

    items
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
                let first_comma = context.get_first_comma_after(&node);
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
    node: Node,
    members: Vec<Node>,
    start_header_info: Option<Info>,
    brace_position: BracePosition,
    should_use_blank_line: Box<dyn Fn(&Node, &Node, &mut Context) -> bool>,
    trailing_commas: Option<TrailingCommas>
}

fn parse_membered_body(opts: ParseMemberedBodyOptions, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();
    let node = opts.node;

    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: opts.brace_position,
        body_node: context.get_first_open_brace_token_within(&node).map(|x| Node::from(x)).unwrap_or(node),
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
            items: opts.members.into_iter().map(|node| (node, Option::None)).collect(),
            last_node: Option::None,
            should_use_space: Option::None,
            should_use_new_line: Option::None,
            should_use_blank_line: opts.should_use_blank_line,
            trailing_commas: opts.trailing_commas,
        }, context));

        items
    }));
    items.push(PrintItem::NewLine);
    items.push("}".into());

    items
}

struct ParseStatementsOrMembersOptions {
    items: Vec<(Node, Option<Vec<PrintItem>>)>,
    last_node: Option<Node>,
    should_use_space: Option<Box<dyn Fn(&Node, &Node, &mut Context) -> bool>>,
    should_use_new_line: Option<Box<dyn Fn(&Node, &Node, &mut Context) -> bool>>,
    should_use_blank_line: Box<dyn Fn(&Node, &Node, &mut Context) -> bool>,
    trailing_commas: Option<TrailingCommas>,
}

fn parse_statements_or_members(opts: ParseStatementsOrMembersOptions, context: &mut Context) -> Vec<PrintItem> {
    let mut last_node = opts.last_node;
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

        last_node = Some(node);
    }

    if let Some(last_node) = last_node {
        items.extend(parse_trailing_comments_as_statements(last_node, context));
    }

    // todo: inner comments?

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

struct ParseBraceSeparatorOptions {
    brace_position: BracePosition,
    body_node: Node,
    start_header_info: Option<Info>,
}

fn parse_brace_separator(opts: ParseBraceSeparatorOptions, context: &mut Context) -> Vec<PrintItem> {
    match opts.brace_position {
        BracePosition::NextLineIfHanging => {
            if let Some(start_header_info) = opts.start_header_info {
                vec![conditions::new_line_if_hanging_space_otherwise(conditions::NewLineIfHangingSpaceOtherwiseOptions {
                    start_info: start_header_info,
                    end_info: Option::None,
                    space_char: Option::None,
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
            vec![] // todo
        },
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

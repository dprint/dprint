extern crate dprint_core;
use std::rc::Rc;
use std::cell::RefCell;

use dprint_core::*;
use dprint_core::{parser_helpers::*,condition_resolvers};
use super::*;
use super::configuration::*;
use swc_ecma_ast::*;
use swc_common::{comments::{Comment, CommentKind}, Spanned, BytePos, Span};
use swc_ecma_parser::{token::{TokenAndSpan}};

// todo: Remove putting functions on heap by using type parameters?

pub fn parse(source_file: ParsedSourceFile, config: Configuration) -> PrintItems {
    let module = Node::Module(&source_file.module);
    let mut context = Context::new(
        config,
        &source_file.leading_comments,
        &source_file.trailing_comments,
        &source_file.tokens,
        &source_file.file_bytes,
        module,
        source_file.info
    );
    let mut items = parse_node(Node::Module(&source_file.module), &mut context);
    items.push_condition(if_true(
        "endOfFileNewLine",
        |context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0),
        Signal::NewLine.into()
    ));
    items
}

fn parse_node<'a>(node: Node<'a>, context: &mut Context<'a>) -> PrintItems {
    parse_node_with_inner_parse(node, context, |items| items)
}

fn parse_node_with_inner_parse<'a>(node: Node<'a>, context: &mut Context<'a>, inner_parse: impl Fn(PrintItems) -> PrintItems + Clone + 'static) -> PrintItems {
    // println!("Node kind: {:?}", node.kind());
    // println!("Text: {:?}", node.text(context));

    // store info
    let past_current_node = std::mem::replace(&mut context.current_node, node.clone());
    let parent_hi = past_current_node.span().hi();
    context.parent_stack.push(past_current_node);

    // parse item
    let node_span = node.span();
    let node_hi = node_span.hi();
    let node_lo = node_span.lo();
    let leading_comments = context.comments.leading_comments_with_previous(node_lo);
    let has_ignore_comment = get_has_ignore_comment(&leading_comments, &node_lo, context);

    let mut items = parse_comments_as_leading(&node_span, leading_comments, context);

    items.extend(if has_ignore_comment {
        parser_helpers::parse_raw_string(&node.text(context))
    } else {
        inner_parse(parse_node_inner(node, context))
    });

    if node_hi != parent_hi || context.parent().kind() == NodeKind::Module {
        let trailing_comments = context.comments.trailing_comments_with_previous(node_hi);
        items.extend(parse_comments_as_trailing(&node_span, trailing_comments, context));
    }

    // pop info
    context.current_node = context.parent_stack.pop();

    return items;

    fn parse_node_inner<'a>(node: Node<'a>, context: &mut Context<'a>) -> PrintItems {
        match node {
            /* class */
            Node::ClassMethod(node) => parse_class_method(node, context),
            Node::ClassProp(node) => parse_class_prop(node, context),
            Node::Constructor(node) => parse_constructor(node, context),
            Node::Decorator(node) => parse_decorator(node, context),
            Node::TsParamProp(node) => parse_parameter_prop(node, context),
            /* clauses */
            Node::CatchClause(node) => parse_catch_clause(node, context),
            /* common */
            Node::ComputedPropName(node) => parse_computed_prop_name(node, context),
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
            Node::OptChainExpr(node) => parse_node((&node.expr).into(), context),
            Node::ParenExpr(node) => parse_paren_expr(node, context),
            Node::SeqExpr(node) => parse_sequence_expr(node, context),
            Node::SetterProp(node) => parse_setter_prop(node, context),
            Node::SpreadElement(node) => parse_spread_element(node, context),
            Node::Super(_) => "super".into(),
            Node::TaggedTpl(node) => parse_tagged_tpl(node, context),
            Node::Tpl(node) => parse_tpl(node, context),
            Node::TplElement(node) => parse_tpl_element(node, context),
            Node::TsAsExpr(node) => parse_as_expr(node, context),
            Node::TsConstAssertion(node) => parse_const_assertion(node, context),
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
            Node::ImportDefault(node) => parse_node((&node.local).into(), context),
            Node::TsExternalModuleRef(node) => parse_external_module_ref(node, context),
            /* interface / type element */
            Node::TsCallSignatureDecl(node) => parse_call_signature_decl(node, context),
            Node::TsConstructSignatureDecl(node) => parse_construct_signature_decl(node, context),
            Node::TsIndexSignature(node) => parse_index_signature(node, context),
            Node::TsInterfaceBody(node) => parse_interface_body(node, context),
            Node::TsMethodSignature(node) => parse_method_signature(node, context),
            Node::TsPropertySignature(node) => parse_property_signature(node, context),
            Node::TsTypeLit(node) => parse_type_lit(node, context),
            /* jsx */
            Node::JSXAttr(node) => parse_jsx_attribute(node, context),
            Node::JSXClosingElement(node) => parse_jsx_closing_element(node, context),
            Node::JSXClosingFragment(node) => parse_jsx_closing_fragment(node, context),
            Node::JSXElement(node) => parse_jsx_element(node, context),
            Node::JSXEmptyExpr(node) => parse_jsx_empty_expr(node, context),
            Node::JSXExprContainer(node) => parse_jsx_expr_container(node, context),
            Node::JSXFragment(node) => parse_jsx_fragment(node, context),
            Node::JSXMemberExpr(node) => parse_jsx_member_expr(node, context),
            Node::JSXNamespacedName(node) => parse_jsx_namespaced_name(node, context),
            Node::JSXOpeningElement(node) => parse_jsx_opening_element(node, context),
            Node::JSXOpeningFragment(node) => parse_jsx_opening_fragment(node, context),
            Node::JSXSpreadChild(node) => parse_jsx_spread_child(node, context),
            Node::JSXText(node) => parse_jsx_text(node, context),
            /* literals */
            Node::BigInt(node) => parse_big_int_literal(node, context),
            Node::Bool(node) => parse_bool_literal(node),
            Node::Null(_) => "null".into(),
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
            Node::ObjectPat(node) => parse_object_pat(node, context),
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
            Node::TsConditionalType(node) => parse_conditional_type(node, context),
            Node::TsConstructorType(node) => parse_constructor_type(node, context),
            Node::TsFnType(node) => parse_function_type(node, context),
            Node::TsImportType(node) => parse_import_type(node, context),
            Node::TsIndexedAccessType(node) => parse_indexed_access_type(node, context),
            Node::TsInferType(node) => parse_infer_type(node, context),
            Node::TsIntersectionType(node) => parse_intersection_type(node, context),
            Node::TsLitType(node) => parse_lit_type(node, context),
            Node::TsMappedType(node) => parse_mapped_type(node, context),
            Node::TsOptionalType(node) => parse_optional_type(node, context),
            Node::TsQualifiedName(node) => parse_qualified_name(node, context),
            Node::TsParenthesizedType(node) => parse_parenthesized_type(node, context),
            Node::TsRestType(node) => parse_rest_type(node, context),
            Node::TsThisType(_) => "this".into(),
            Node::TsTupleType(node) => parse_tuple_type(node, context),
            Node::TsTypeAnn(node) => parse_type_ann(node, context),
            Node::TsTypeParam(node) => parse_type_param(node, context),
            Node::TsTypeParamDecl(node) => parse_type_param_instantiation(TypeParamNode::Decl(node), context),
            Node::TsTypeParamInstantiation(node) => parse_type_param_instantiation(TypeParamNode::Instantiation(node), context),
            Node::TsTypeOperator(node) => parse_type_operator(node, context),
            Node::TsTypePredicate(node) => parse_type_predicate(node, context),
            Node::TsTypeQuery(node) => parse_type_query(node, context),
            Node::TsTypeRef(node) => parse_type_reference(node, context),
            Node::TsUnionType(node) => parse_union_type(node, context),
            /* unknown */
            _ => parse_raw_string(node.text(context).into()),
        }
    }

    fn get_has_ignore_comment<'a>(leading_comments: &CommentsIterator<'a>, node_lo: &BytePos, context: &mut Context<'a>) -> bool {
        if let Some(last_comment) = get_last_comment(leading_comments, node_lo, context) {
            let searching_text = "dprint-ignore";
            let pos = last_comment.text.find(searching_text);
            if let Some(pos) = pos {
                let end = pos + searching_text.len();
                if pos > 0 && is_alpha_numeric_at_pos(&last_comment.text, pos - 1) {
                    return false;
                }
                if is_alpha_numeric_at_pos(&last_comment.text, end) {
                    return false;
                }
                return true;
            }
        }

        return false;

        fn get_last_comment<'a>(leading_comments: &CommentsIterator<'a>, node_lo: &BytePos, context: &mut Context<'a>) -> Option<&'a Comment> {
            return match context.parent() {
                Node::JSXElement(jsx_element) => get_last_comment_for_jsx_children(&jsx_element.children, node_lo, context),
                Node::JSXFragment(jsx_fragment) => get_last_comment_for_jsx_children(&jsx_fragment.children, node_lo, context),
                _ => leading_comments.get_last_comment(),
            };

            fn get_last_comment_for_jsx_children<'a>(children: &Vec<JSXElementChild>, node_lo: &BytePos, context: &mut Context<'a>) -> Option<&'a Comment> {
                let index = children.binary_search_by_key(node_lo, |child| child.lo()).ok()?;
                for i in (0..index).rev() {
                    match children.get(i)? {
                        JSXElementChild::JSXExprContainer(expr_container) => {
                            return match expr_container.expr {
                                JSXExpr::JSXEmptyExpr(empty_expr) => {
                                    get_jsx_empty_expr_comments(&empty_expr, context).last()
                                },
                                _ => None,
                            };
                        },
                        JSXElementChild::JSXText(jsx_text) => {
                            if !jsx_text.text(context).trim().is_empty() { return None; }
                        }
                        _=> return None,
                    }
                }

                None
            }
        }

        fn is_alpha_numeric_at_pos(text: &String, pos: usize) -> bool {
            if let Some(chars_after) = text.get(pos..) {
                if let Some(char_after) = chars_after.chars().next() {
                    return char_after.is_alphanumeric();
                }
            }
            return false;
        }
    }
}

/* class */

fn parse_class_method<'a>(node: &'a ClassMethod, context: &mut Context<'a>) -> PrintItems {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: Some(&node.function.decorators),
        accessibility: node.accessibility,
        is_static: node.is_static,
        is_async: node.function.is_async,
        is_abstract: node.is_abstract,
        kind: node.kind.into(),
        is_generator: node.function.is_generator,
        is_optional: node.is_optional,
        key: (&node.key).into(),
        type_params: node.function.type_params.as_ref().map(|x| x.into()),
        params: node.function.params.iter().map(|x| x.into()).collect(),
        return_type: node.function.return_type.as_ref().map(|x| x.into()),
        body: node.function.body.as_ref().map(|x| x.into()),
    }, context);
}

fn parse_class_prop<'a>(node: &'a ClassProp, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_decorators(&node.decorators, false, context));
    if let Some(accessibility) = node.accessibility {
        items.push_str(&format!("{} ", accessibility_to_str(&accessibility)));
    }
    if node.is_static { items.push_str("static "); }
    if node.is_abstract { items.push_str("abstract "); }
    if node.readonly { items.push_str("readonly "); }
    if node.computed { items.push_str("["); }
    items.extend(parse_node((&node.key).into(), context));
    if node.computed { items.push_str("]"); }
    if node.is_optional { items.push_str("?"); }
    if node.definite { items.push_str("!"); }
    items.extend(parse_type_annotation_with_colon_if_exists(&node.type_ann, context));

    if let Some(value) = &node.value {
        items.push_str(" = ");
        items.extend(parse_node(value.into(), context));
    }

    if context.config.class_property_semi_colon {
        items.push_str(";");
    }

    return items;
}

fn parse_constructor<'a>(node: &'a Constructor, context: &mut Context<'a>) -> PrintItems {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: None,
        accessibility: node.accessibility,
        is_static: false,
        is_async: false,
        is_abstract: false,
        kind: ClassOrObjectMethodKind::Constructor,
        is_generator: false,
        is_optional: node.is_optional,
        key: (&node.key).into(),
        type_params: None,
        params: node.params.iter().map(|x| x.into()).collect(),
        return_type: None,
        body: node.body.as_ref().map(|x| x.into()),
    }, context);
}

fn parse_decorator<'a>(node: &'a Decorator, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("@");
    items.extend(parse_node((&node.expr).into(), context));
    return items;
}

fn parse_parameter_prop<'a>(node: &'a TsParamProp, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_decorators(&node.decorators, true, context));
    if let Some(accessibility) = node.accessibility {
        items.push_str(&format!("{} ", accessibility_to_str(&accessibility)));
    }
    if node.readonly { items.push_str("readonly "); }
    items.extend(parse_node((&node.param).into(), context));
    return items;
}

/* clauses */

fn parse_catch_clause<'a>(node: &'a CatchClause, context: &mut Context<'a>) -> PrintItems {
    // a bit overkill since the param will currently always just be an identifer
    let start_header_info = Info::new("catchClauseHeaderStart");
    let end_header_info = Info::new("catchClauseHeaderEnd");
    let mut items = PrintItems::new();

    items.push_info(start_header_info);
    items.push_str("catch");

    if let Some(param) = &node.param {
        items.push_str(" (");
        items.extend(parse_node(param.into(), context));
        items.push_str(")");
    }
    items.push_info(end_header_info);

    // not conditional... required
    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: (&node.body).into(),
        use_braces: UseBraces::Always,
        brace_position: context.config.try_statement_brace_position,
        single_body_position: None,
        requires_braces_condition_ref: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);

    return items;
}

/* common */

fn parse_computed_prop_name<'a>(node: &'a ComputedPropName, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("[");
    items.extend(parse_node((&node.expr).into(), context));
    items.push_str("]");
    return items;
}

fn parse_identifier<'a>(node: &'a Ident, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str(&node.sym as &str);

    if node.optional {
        items.push_str("?");
    }
    if let Node::VarDeclarator(node) = context.parent() {
        if node.definite {
            items.push_str("!");
        }
    }

    items.extend(parse_type_annotation_with_colon_if_exists(&node.type_ann, context));

    return items;
}

/* declarations */

fn parse_class_decl<'a>(node: &'a ClassDecl, context: &mut Context<'a>) -> PrintItems {
    return parse_class_decl_or_expr(ClassDeclOrExpr {
        span: node.class.span,
        decorators: &node.class.decorators,
        is_class_expr: false,
        is_declare: node.declare,
        is_abstract: node.class.is_abstract,
        ident: Some((&node.ident).into()),
        type_params: node.class.type_params.as_ref().map(|x| x.into()),
        super_class: node.class.super_class.as_ref().map(|x| x.into()),
        super_type_params: node.class.super_type_params.as_ref().map(|x| x.into()),
        implements: node.class.implements.iter().map(|x| x.into()).collect(),
        members: node.class.body.iter().map(|x| x.into()).collect(),
        brace_position: context.config.class_declaration_brace_position,
    }, context);
}

struct ClassDeclOrExpr<'a> {
    span: Span,
    decorators: &'a Vec<Decorator>,
    is_class_expr: bool,
    is_declare: bool,
    is_abstract: bool,
    ident: Option<Node<'a>>,
    type_params: Option<Node<'a>>,
    super_class: Option<Node<'a>>,
    super_type_params: Option<Node<'a>>,
    implements: Vec<Node<'a>>,
    members: Vec<Node<'a>>,
    brace_position: BracePosition,
}

fn parse_class_decl_or_expr<'a>(node: ClassDeclOrExpr<'a>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    let parent_kind = context.parent().kind();
    if parent_kind != NodeKind::ExportDecl && parent_kind != NodeKind::ExportDefaultDecl {
        items.extend(parse_decorators(node.decorators, node.is_class_expr, context));
    }
    let start_header_info = Info::new("startHeader");
    let parsed_header = {
        let mut items = PrintItems::new();
        items.push_info(start_header_info);

        if node.is_declare { items.push_str("declare "); }
        if node.is_abstract { items.push_str("abstract "); }

        items.push_str("class");

        if let Some(ident) = node.ident {
            items.push_str(" ");
            items.extend(parse_node(ident, context));
        }
        if let Some(type_params) = node.type_params {
            items.extend(parse_node(type_params, context));
        }
        if let Some(super_class) = node.super_class {
            items.push_condition(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_header_info, None));
            items.push_condition(conditions::indent_if_start_of_line({
                let mut items = PrintItems::new();
                items.push_str("extends ");
                items.extend(parse_node(super_class, context));
                if let Some(super_type_params) = node.super_type_params {
                    items.extend(parse_node(super_type_params, context));
                }
                items
            }));
        }
        items.extend(parse_extends_or_implements("implements", node.implements, start_header_info, context));
        items
    };

    if node.is_class_expr {
        items.push_condition(conditions::indent_if_start_of_line(parsed_header));
    } else {
        items.extend(parsed_header);
    }

    // parse body
    items.extend(parse_membered_body(ParseMemberedBodyOptions {
        span: node.span,
        members: node.members,
        start_header_info: Some(start_header_info),
        brace_position: node.brace_position,
        should_use_blank_line: move |previous, next, context| {
            node_helpers::has_separating_blank_line(previous, next, context)
        },
        trailing_commas: None,
    }, context));

    return items;
}

fn parse_export_decl<'a>(node: &'a ExportDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if let Decl::Class(class_decl) = &node.decl {
        items.extend(parse_decorators(&class_decl.class.decorators, false, context));
    }
    items.push_str("export ");
    items.extend(parse_node((&node.decl).into(), context));
    items
}

fn parse_export_default_decl<'a>(node: &'a ExportDefaultDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if let DefaultDecl::Class(class_expr) = &node.decl {
        items.extend(parse_decorators(&class_expr.class.decorators, false, context));
    }
    items.push_str("export default ");
    items.extend(parse_node((&node.decl).into(), context));
    items
}

fn parse_export_default_expr<'a>(node: &'a ExportDefaultExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("export default ");
    items.extend(parse_node((&node.expr).into(), context));
    if context.config.export_default_expression_semi_colon { items.push_str(";"); }
    items
}

fn parse_enum_decl<'a>(node: &'a TsEnumDecl, context: &mut Context<'a>) -> PrintItems {
    let start_header_info = Info::new("startHeader");
    let mut items = PrintItems::new();

    // header
    items.push_info(start_header_info);

    if node.declare { items.push_str("declare "); }
    if node.is_const { items.push_str("const "); }
    items.push_str("enum ");
    items.extend(parse_node((&node.id).into(), context));

    // body
    let member_spacing = context.config.enum_declaration_member_spacing;
    items.extend(parse_membered_body(ParseMemberedBodyOptions {
        span: node.span,
        members: node.members.iter().map(|x| x.into()).collect(),
        start_header_info: Some(start_header_info),
        brace_position: context.config.enum_declaration_brace_position,
        should_use_blank_line: move |previous, next, context| {
            match member_spacing {
                MemberSpacing::BlankLine => true,
                MemberSpacing::NewLine => false,
                MemberSpacing::Maintain => node_helpers::has_separating_blank_line(previous, next, context),
            }
        },
        trailing_commas: Some(context.config.enum_declaration_trailing_commas),
    }, context));

    return items;
}

fn parse_enum_member<'a>(node: &'a TsEnumMember, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.id).into(), context));

    if let Some(init) = &node.init {
        match init.kind() {
            NodeKind::Number | NodeKind::Str => items.push_signal(Signal::SpaceOrNewLine),
            _ => items.push_str(" "),
        };

        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str("= ");
            items.extend(parse_node(init.into(), context));
            items
        }));
    }

    items
}

fn parse_export_named_decl<'a>(node: &'a NamedExport, context: &mut Context<'a>) -> PrintItems {
    // fill specifiers
    let mut default_export: Option<&DefaultExportSpecifier> = None;
    let mut namespace_export: Option<&NamespaceExportSpecifier> = None;
    let mut named_exports: Vec<&NamedExportSpecifier> = Vec::new();

    for specifier in &node.specifiers {
        match specifier {
            ExportSpecifier::Default(node) => default_export = Some(node),
            ExportSpecifier::Namespace(node) => namespace_export = Some(node),
            ExportSpecifier::Named(node) => named_exports.push(node),
        }
    }

    // parse
    let mut items = PrintItems::new();

    items.push_str("export ");

    if let Some(default_export) = default_export {
        items.extend(parse_node(default_export.into(), context));
    } else if !named_exports.is_empty() {
        items.extend(parse_named_import_or_export_specifiers(
            NamedImportOrExportDeclaration::Export(node),
            named_exports.into_iter().map(|x| x.into()).collect(),
            context
        ));
    } else if let Some(namespace_export) = namespace_export {
        items.extend(parse_node(namespace_export.into(), context));
    } else {
        items.push_str("{}");
    }

    if let Some(src) = &node.src {
        items.push_str(" from ");
        items.extend(parse_node(src.into(), context));
    }

    if context.config.export_named_declaration_semi_colon {
        items.push_str(";");
    }

    items
}

fn parse_function_decl<'a>(node: &'a FnDecl, context: &mut Context<'a>) -> PrintItems {
    parse_function_decl_or_expr(FunctionDeclOrExprNode {
        is_func_decl: true,
        ident: Some(&node.ident),
        declare: node.declare,
        func: &node.function,
    }, context)
}

struct FunctionDeclOrExprNode<'a> {
    is_func_decl: bool,
    ident: Option<&'a Ident>,
    declare: bool,
    func: &'a Function,
}

fn parse_function_decl_or_expr<'a>(node: FunctionDeclOrExprNode<'a>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let start_header_info = Info::new("functionHeaderStart");
    let func = node.func;

    items.push_info(start_header_info);
    if node.declare { items.push_str("declare "); }
    if func.is_async { items.push_str("async "); }
    items.push_str("function");
    if func.is_generator { items.push_str("*"); }
    if let Some(ident) = node.ident {
        items.push_str(" ");
        items.extend(parse_node(ident.into(), context));
    }
    if let Some(type_params) = &func.type_params { items.extend(parse_node(type_params.into(), context)); }
    if get_use_space_before_parens(node.is_func_decl, context) { items.push_str(" "); }

    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: func.params.iter().map(|node| node.into()).collect(),
        prefer_hanging: if node.is_func_decl {
            context.config.function_declaration_prefer_hanging_parameters
        } else {
            context.config.function_expression_prefer_hanging_parameters
        },
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info: start_header_info,
            type_node: func.return_type.as_ref().map(|x| x.into()),
            type_node_separator: None,
        }, context)),
    }, context));

    if let Some(body) = &func.body {
        let brace_position = if node.is_func_decl {
            context.config.function_declaration_brace_position
        } else {
            context.config.function_expression_brace_position
        };
        let open_brace_token = context.token_finder.get_first_open_brace_token_within(&body);

        items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
            brace_position: brace_position,
            open_brace_token: open_brace_token,
            start_header_info: Some(start_header_info),
        }, context));

        items.extend(parse_node(body.into(), context));
    } else {
        if context.config.function_declaration_semi_colon {
            items.push_str(";");
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

fn parse_import_decl<'a>(node: &'a ImportDecl, context: &mut Context<'a>) -> PrintItems {
    // fill specifiers
    let mut default_import: Option<&ImportDefault> = None;
    let mut namespace_import: Option<&ImportStarAs> = None;
    let mut named_imports: Vec<&ImportSpecific> = Vec::new();

    for specifier in &node.specifiers {
        match specifier {
            ImportSpecifier::Default(node) => default_import = Some(node),
            ImportSpecifier::Namespace(node) => namespace_import = Some(node),
            ImportSpecifier::Specific(node) => named_imports.push(node),
        }
    }

    let mut items = PrintItems::new();
    let has_from = default_import.is_some() || namespace_import.is_some() || !named_imports.is_empty();
    items.push_str("import ");

    if let Some(default_import) = default_import {
        items.extend(parse_node(default_import.into(), context));
        if namespace_import.is_some() || !named_imports.is_empty() {
            items.push_str(", ");
        }
    }
    if let Some(namespace_import) = namespace_import {
        items.extend(parse_node(namespace_import.into(), context));
    }
    items.extend(parse_named_import_or_export_specifiers(
        NamedImportOrExportDeclaration::Import(node),
        named_imports.into_iter().map(|x| x.into()).collect(),
        context
    ));

    if has_from { items.push_str(" from "); }

    items.extend(parse_node((&node.src).into(), context));

    if context.config.import_declaration_semi_colon {
        items.push_str(";");
    }

    return items;
}

fn parse_import_equals_decl<'a>(node: &'a TsImportEqualsDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if node.is_export {
        items.push_str("export ");
    }

    items.push_str("import ");
    items.extend(parse_node((&node.id).into(), context));
    items.push_str(" = ");
    items.extend(parse_node((&node.module_ref).into(), context));

    if context.config.import_equals_declaration_semi_colon { items.push_str(";"); }

    return items;
}

fn parse_interface_decl<'a>(node: &'a TsInterfaceDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let start_header_info = Info::new("startHeader");
    items.push_info(start_header_info);
    context.store_info_for_node(&node, start_header_info);

    if node.declare { items.push_str("declare "); }
    items.push_str("interface ");
    items.extend(parse_node((&node.id).into(), context));
    if let Some(type_params) = &node.type_params { items.extend(parse_node(type_params.into(), context)); }
    items.extend(parse_extends_or_implements("extends", node.extends.iter().map(|x| x.into()).collect(), start_header_info, context));
    items.extend(parse_node((&node.body).into(), context));

    return items;
}

fn parse_module_decl<'a>(node: &'a TsModuleDecl, context: &mut Context<'a>) -> PrintItems {
    parse_module_or_namespace_decl(ModuleOrNamespaceDecl {
        span: node.span,
        declare: node.declare,
        global: node.global,
        id: (&node.id).into(),
        body: node.body.as_ref(),
    }, context)
}

fn parse_namespace_decl<'a>(node: &'a TsNamespaceDecl, context: &mut Context<'a>) -> PrintItems {
    parse_module_or_namespace_decl(ModuleOrNamespaceDecl {
        span: node.span,
        declare: node.declare,
        global: node.global,
        id: (&node.id).into(),
        body: Some(&node.body)
    }, context)
}

struct ModuleOrNamespaceDecl<'a> {
    pub span: Span,
    pub declare: bool,
    pub global: bool,
    pub id: Node<'a>,
    pub body: Option<&'a TsNamespaceBody>,
}

fn parse_module_or_namespace_decl<'a>(node: ModuleOrNamespaceDecl<'a>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    let start_header_info = Info::new("startHeader");
    items.push_info(start_header_info);

    if node.declare { items.push_str("declare "); }
    if node.global {
        items.push_str("global");
        // items.extend(parse_node(node.id.into(), context));
    } else {
        let has_namespace_keyword = context.token_finder.get_char_at(&node.span.lo()) == 'n';
        items.push_str(if has_namespace_keyword { "namespace " } else { "module " });
    }

    items.extend(parse_node(node.id.into(), context));
    items.extend(parse_body(node.body, start_header_info, context));

    return items;

    fn parse_body<'a>(body: Option<&'a TsNamespaceBody>, start_header_info: Info, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        if let Some(body) = &body {
            match body {
                TsNamespaceBody::TsModuleBlock(block) => {
                    items.extend(parse_membered_body(ParseMemberedBodyOptions {
                        span: block.span,
                        members: block.body.iter().map(|x| x.into()).collect(),
                        start_header_info: Some(start_header_info),
                        brace_position: context.config.module_declaration_brace_position,
                        should_use_blank_line: move |previous, next, context| {
                            node_helpers::has_separating_blank_line(previous, next, context)
                        },
                        trailing_commas: None,
                    }, context));
                },
                TsNamespaceBody::TsNamespaceDecl(decl) => {
                    items.push_str(".");
                    items.extend(parse_node((&decl.id).into(), context));
                    items.extend(parse_body(Some(&*decl.body), start_header_info, context));
                }
            }
        }
        else if context.config.module_declaration_semi_colon {
            items.push_str(";");
        }

        return items;
    }
}

fn parse_type_alias<'a>(node: &'a TsTypeAliasDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if node.declare { items.push_str("declare "); }
    items.push_str("type ");
    items.extend(parse_node((&node.id).into(), context));
    if let Some(type_params) = &node.type_params {
        items.extend(parse_node(type_params.into(), context));
    }
    items.push_str(" = ");
    items.extend(parse_node((&node.type_ann).into(), context));

    if context.config.type_alias_semi_colon { items.push_str(";"); }

    return items;
}

/* exports */

fn parse_named_import_or_export_specifiers<'a>(parent_decl: NamedImportOrExportDeclaration<'a>, specifiers: Vec<Node<'a>>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if specifiers.is_empty() {
        return items;
    }

    let use_space = get_use_space(&parent_decl, context);
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(
        &context.token_finder.get_first_open_brace_token_within(&Node::from(parent_decl)),
        &specifiers[0],
        context
    );
    let brace_separator: PrintItems = if use_new_lines { Signal::NewLine.into() } else if use_space { " ".into() } else { "".into() };
    let brace_separator = brace_separator.into_rc_path();

    items.push_str("{");
    items.extend(brace_separator.clone().into());

    let specifiers = {
        let mut items = PrintItems::new();
        for (i, specifier) in specifiers.into_iter().enumerate() {
            if i > 0 {
                items.push_str(",");
                items.push_signal(if use_new_lines { Signal::NewLine } else { Signal::SpaceOrNewLine });
            }

            let parsed_specifier = parse_node(specifier.into(), context);
            if use_new_lines {
                items.extend(parsed_specifier)
            } else {
                items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parsed_specifier)));
            }
        }
        items
    };

    items.extend(if use_new_lines { parser_helpers::with_indent(specifiers) } else { specifiers });

    items.extend(brace_separator.into());
    items.push_str("}");

    return items;

    fn get_use_space(parent_decl: &NamedImportOrExportDeclaration, context: &mut Context) -> bool {
        match parent_decl {
            NamedImportOrExportDeclaration::Export(_) => context.config.export_declaration_space_surrounding_named_exports,
            NamedImportOrExportDeclaration::Import(_) => context.config.import_declaration_space_surrounding_named_imports,
        }
    }
}

/* expressions */

fn parse_array_expr<'a>(node: &'a ArrayLit, context: &mut Context<'a>) -> PrintItems {
    parse_array_like_nodes(ParseArrayLikeNodesOptions {
        parent_span: node.span,
        elements: node.elems.iter().map(|x| x.as_ref().map(|elem| elem.into())).collect(),
        trailing_commas: context.config.array_expression_trailing_commas,
    }, context)
}

fn parse_arrow_func_expr<'a>(node: &'a ArrowExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let header_start_info = Info::new("arrowFunctionExpressionHeaderStart");
    let should_use_params = get_should_use_params(&node, context);

    items.push_info(header_start_info);
    if node.is_async { items.push_str("async "); }
    if let Some(type_params) = &node.type_params { items.extend(parse_node(type_params.into(), context)); }

    if should_use_params {
        items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
            nodes: node.params.iter().map(|node| node.into()).collect(),
            prefer_hanging: context.config.arrow_function_expression_prefer_hanging_parameters,
            custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
                start_info: header_start_info,
                type_node: node.return_type.as_ref().map(|x| x.into()),
                type_node_separator: None,
            }, context)),
        }, context));
    } else {
        items.extend(parse_node(node.params.iter().next().unwrap().into(), context));
    }

    items.push_str(" =>");

    let open_brace_token = match &node.body {
        BlockStmtOrExpr::BlockStmt(stmt) => context.token_finder.get_first_open_brace_token_within(&stmt),
        _ => None,
    };
    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: context.config.arrow_function_expression_brace_position,
        open_brace_token: open_brace_token,
        start_header_info: Some(header_start_info),
    }, context));

    items.extend(parse_node((&node.body).into(), context));

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
                context.token_finder.get_char_at(&node.lo()) == '('
            }
        }
    }
}

fn parse_as_expr<'a>(node: &'a TsAsExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.expr).into(), context));
    items.push_str(" as ");
    items.push_condition(conditions::with_indent_if_start_of_line_indented(parse_node((&node.type_ann).into(), context)));
    items
}

fn parse_const_assertion<'a>(node: &'a TsConstAssertion, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.expr).into(), context));
    items.push_str(" as const");
    items
}

fn parse_assignment_expr<'a>(node: &'a AssignExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.left).into(), context));
    items.push_str(&format!(" {} ", node.op));
    items.push_condition(conditions::with_indent_if_start_of_line_indented(parse_node((&node.right).into(), context)));
    items
}

fn parse_await_expr<'a>(node: &'a AwaitExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("await ");
    items.extend(parse_node((&node.arg).into(), context));
    items
}

fn parse_binary_expr<'a>(node: &'a BinExpr, context: &mut Context<'a>) -> PrintItems {
    return if is_expression_breakable(&node.op) {
        inner_parse(node, context)
    } else {
        new_line_group(inner_parse(node, context))
    };

    fn inner_parse<'a>(node: &'a BinExpr, context: &mut Context<'a>) -> PrintItems {
        // todo: clean this up
        let operator_token = context.token_finder.get_first_operator_after(&node.left, node.op.as_str()).unwrap();
        let operator_position = get_operator_position(&node, &operator_token, context);
        let top_most_expr_start = get_top_most_binary_expr_pos(&node, context);
        let node_left = &*node.left;
        let node_right = &*node.right;
        let node_op = node.op;
        let use_space_surrounding_operator = get_use_space_surrounding_operator(&node_op, context);
        let is_top_most = top_most_expr_start == node.lo();
        let use_new_lines = node_helpers::get_use_new_lines_for_nodes(node_left, node_right, context);
        let top_most_info = get_or_set_top_most_info(top_most_expr_start, is_top_most, context);
        let indent_disabled = context.get_disable_indent_for_next_bin_expr();
        let mut items = PrintItems::new();

        if is_top_most {
            items.push_info(top_most_info);
        }

        let node_left_node = Node::from(node_left);
        if indent_disabled && node_left_node.kind() == NodeKind::BinExpr {
            context.mark_disable_indent_for_next_bin_expr();
        }

        items.extend(indent_if_necessary(node_left.lo(), top_most_expr_start, top_most_info, indent_disabled, {
            new_line_group_if_necessary(&node_left, parse_node_with_inner_parse(node_left_node, context, move |mut items| {
                if operator_position == OperatorPosition::SameLine {
                    if use_space_surrounding_operator {
                        items.push_str(" ");
                    }
                    items.push_str(node_op.as_str());
                }
                items
            }))
        }));

        items.extend(parse_comments_as_trailing(&operator_token, operator_token.trailing_comments(context), context));

        items.push_signal(if use_new_lines {
            Signal::NewLine
        } else if use_space_surrounding_operator {
            Signal::SpaceOrNewLine
        } else {
            Signal::PossibleNewLine
        });

        let node_right_node = Node::from(node_right);
        if indent_disabled && node_right_node.kind() == NodeKind::BinExpr {
            context.mark_disable_indent_for_next_bin_expr();
        }

        items.extend(indent_if_necessary(node_right.lo(), top_most_expr_start, top_most_info, indent_disabled, {
            let mut items = PrintItems::new();
            let use_new_line_group = get_use_new_line_group(&node_right);
            items.extend(parse_comments_as_leading(node_right, operator_token.leading_comments(context), context));
            items.extend(parse_node_with_inner_parse(node_right.into(), context, move |items| {
                let mut new_items = PrintItems::new();
                if operator_position == OperatorPosition::NextLine {
                    new_items.push_str(node_op.as_str());
                    if use_space_surrounding_operator {
                        new_items.push_str(" ");
                    }
                }
                new_items.extend(if use_new_line_group { new_line_group(items) } else { items });
                new_items
            }));
            items
        }));

        return items;
    }

    fn indent_if_necessary(
        current_node_start: BytePos,
        top_most_expr_start: BytePos,
        top_most_info: Info,
        indent_disabled: bool,
        items: PrintItems
    ) -> PrintItems {
        let is_left_most_node = top_most_expr_start == current_node_start;
        let items = items.into_rc_path();
        Condition::new("indentIfNecessaryForBinaryExpressions", ConditionProperties {
            condition: Box::new(move |condition_context| {
                if indent_disabled || is_left_most_node { return Some(false); }
                let top_most_info = condition_context.get_resolved_info(&top_most_info)?;
                let is_same_indent = top_most_info.indent_level == condition_context.writer_info.indent_level;
                return Some(is_same_indent && condition_resolvers::is_start_of_new_line(condition_context));
            }),
            true_path: Some(parser_helpers::with_indent(items.clone().into())),
            false_path: Some(items.into())
        }).into()
    }

    fn new_line_group_if_necessary(expr: &Expr, items: PrintItems) -> PrintItems {
        match get_use_new_line_group(expr) {
            true => parser_helpers::new_line_group(items),
            false => items,
        }
    }

    fn get_use_new_line_group(expr: &Expr) -> bool {
        match expr {
            Expr::Bin(_) => false,
            _ => true,
        }
    }

    fn get_or_set_top_most_info(top_most_expr_start: BytePos, is_top_most: bool, context: &mut Context) -> Info {
        if is_top_most {
            let info = Info::new("topBinaryOrLogicalExpressionStart");
            context.store_info_for_node(&top_most_expr_start, info);
            return info;
        }
        return context.get_info_for_node(&top_most_expr_start).expect("Expected to have the top most expr info stored");
    }

    fn get_top_most_binary_expr_pos(node: &BinExpr, context: &mut Context) -> BytePos {
        let mut top_most: &BinExpr = node;
        if is_expression_breakable(&node.op) {
            for ancestor in context.parent_stack.iter() {
                if let Node::BinExpr(ancestor) = ancestor {
                    if is_expression_breakable(&ancestor.op) {
                        top_most = ancestor;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        return top_most.lo();
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

fn parse_call_expr<'a>(node: &'a CallExpr, context: &mut Context<'a>) -> PrintItems {
    return if is_test_library_call_expr(&node, context) {
        parse_test_library_call_expr(node, context)
    } else {
        inner_parse(node, context)
    };

    fn inner_parse<'a>(node: &'a CallExpr, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();

        items.extend(parse_node((&node.callee).into(), context));

        if let Some(type_args) = &node.type_args {
            items.extend(parse_node(type_args.into(), context));
        }

        if is_optional(context) {
            items.push_str("?.");
        }

        items.push_condition(conditions::with_indent_if_start_of_line_indented(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
            nodes: node.args.iter().map(|node| node.into()).collect(),
            prefer_hanging: context.config.call_expression_prefer_hanging_arguments,
            custom_close_paren: None,
        }, context)));

        items
    }

    fn parse_test_library_call_expr<'a>(node: &'a CallExpr, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        items.extend(parse_test_library_callee(&node.callee, context));
        items.extend(parse_test_library_arguments(&node.args, context));
        return items;

        fn parse_test_library_callee<'a>(callee: &'a ExprOrSuper, context: &mut Context<'a>) -> PrintItems {
            match callee {
                ExprOrSuper::Expr(expr) => {
                    let expr = &**expr;
                    match expr {
                        Expr::Member(member_expr) => {
                            let mut items = PrintItems::new();
                            items.extend(parse_node((&member_expr.obj).into(), context));
                            items.push_str(".");
                            items.extend(parse_node((&member_expr.prop).into(), context));
                            items
                        },
                        _=> parse_node(expr.into(), context),
                    }
                },
                _ => parse_node(callee.into(), context),
            }
        }

        fn parse_test_library_arguments<'a>(args: &'a Vec<ExprOrSpread>, context: &mut Context<'a>) -> PrintItems {
            let mut items = PrintItems::new();
            items.push_str("(");
            items.extend(parse_node_with_inner_parse((&args[0]).into(), context, |items| {
                let mut new_items = filter_signals(items);
                new_items.push_str(",");
                new_items
            }));
            items.push_str(" ");
            items.extend(parse_node((&args[1]).into(), context));
            items.push_str(")");

            return items;
        }

        pub fn filter_signals(old_items: PrintItems) -> PrintItems {
            let mut items = PrintItems::new();
            for item in old_items.iter() {
                match item {
                    PrintItem::String(_) | PrintItem::Condition(_) | PrintItem::Info(_) | PrintItem::RcPath(_) => items.push_item(item),
                    PrintItem::Signal(_) => {},
                }
            }
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
                    ExprOrSuper::Expr(expr) => {
                        match &**expr {
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

fn parse_class_expr<'a>(node: &'a ClassExpr, context: &mut Context<'a>) -> PrintItems {
    return parse_class_decl_or_expr(ClassDeclOrExpr {
        span: node.class.span,
        decorators: &node.class.decorators,
        is_class_expr: true,
        is_declare: false,
        is_abstract: node.class.is_abstract,
        ident: node.ident.as_ref().map(|x| x.into()),
        type_params: node.class.type_params.as_ref().map(|x| x.into()),
        super_class: node.class.super_class.as_ref().map(|x| x.into()),
        super_type_params: node.class.super_type_params.as_ref().map(|x| x.into()),
        implements: node.class.implements.iter().map(|x| x.into()).collect(),
        members: node.class.body.iter().map(|x| x.into()).collect(),
        brace_position: context.config.class_expression_brace_position,
    }, context);
}

fn parse_conditional_expr<'a>(node: &'a CondExpr, context: &mut Context<'a>) -> PrintItems {
    let operator_token = context.token_finder.get_first_operator_after(&node.test, "?").unwrap();
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&node.test, &node.cons, context)
        || node_helpers::get_use_new_lines_for_nodes(&node.cons, &node.alt, context);
    let operator_position = get_operator_position(&node, &operator_token, context);
    let start_info = Info::new("startConditionalExpression");
    let before_alternate_info = Info::new("beforeAlternateInfo");
    let end_info = Info::new("endConditionalExpression");
    let mut items = PrintItems::new();

    items.push_info(start_info);
    items.extend(parser_helpers::new_line_group(parse_node_with_inner_parse((&node.test).into(), context, {
        move |mut items| {
            if operator_position == OperatorPosition::SameLine {
                items.push_str(" ?");
            }
            items
        }
    })));

    // force re-evaluation of all the conditions below once the end info has been reached
    items.push_condition(conditions::force_reevaluation_once_resolved(context.end_statement_or_member_infos.peek().map(|x| x.clone()).unwrap_or(end_info)));

    if use_new_lines {
        items.push_signal(Signal::NewLine);
    } else {
        items.push_condition(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_info, Some(before_alternate_info)));
    }

    items.push_condition(conditions::indent_if_start_of_line({
        let mut items = PrintItems::new();
        if operator_position == OperatorPosition::NextLine {
            items.push_str("? ");
        }
        items.extend(parser_helpers::new_line_group(parse_node_with_inner_parse((&node.cons).into(), context, {
            move |mut items| {
                if operator_position == OperatorPosition::SameLine {
                    items.push_str(" :");
                }
                items
            }
        })));
        items
    }));

    if use_new_lines {
        items.push_signal(Signal::NewLine);
    } else {
        items.push_condition(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_info, Some(before_alternate_info)));
    }

    items.push_condition(conditions::indent_if_start_of_line({
        let mut items = PrintItems::new();
        if operator_position == OperatorPosition::NextLine {
            items.push_str(": ");
        }
        items.push_info(before_alternate_info);
        items.extend(parser_helpers::new_line_group(parse_node((&node.alt).into(), context)));
        items.push_info(end_info);
        items
    }));

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

fn parse_expr_or_spread<'a>(node: &'a ExprOrSpread, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if node.spread.is_some() { items.push_str("..."); }
    items.extend(parse_node((&node.expr).into(), context));
    items
}

fn parse_expr_with_type_args<'a>(node: &'a TsExprWithTypeArgs, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.expr).into(), context));
    if let Some(type_args) = &node.type_args {
        items.extend(parse_node(type_args.into(), context));
    }
    return items;
}

fn parse_fn_expr<'a>(node: &'a FnExpr, context: &mut Context<'a>) -> PrintItems {
    parse_function_decl_or_expr(FunctionDeclOrExprNode {
        is_func_decl: false,
        ident: node.ident.as_ref(),
        declare: false,
        func: &node.function,
    }, context)
}

fn parse_getter_prop<'a>(node: &'a GetterProp, context: &mut Context<'a>) -> PrintItems {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: None,
        accessibility: None,
        is_static: false,
        is_async: false,
        is_abstract: false,
        kind: ClassOrObjectMethodKind::Getter,
        is_generator: false,
        is_optional: false,
        key: (&node.key).into(),
        type_params: None,
        params: Vec::new(),
        return_type: node.type_ann.as_ref().map(|x| x.into()),
        body: node.body.as_ref().map(|x| x.into()),
    }, context);
}

fn parse_key_value_prop<'a>(node: &'a KeyValueProp, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.key).into(), context));
    items.extend(parse_node_with_preceeding_colon(Some((&node.value).into()), context));
    return items;
}

fn parse_member_expr<'a>(node: &'a MemberExpr, context: &mut Context<'a>) -> PrintItems {
    return parse_for_member_like_expr(MemberLikeExpr {
        left_node: (&node.obj).into(),
        right_node: (&node.prop).into(),
        is_computed: node.computed,
    }, context);
}

fn parse_meta_prop_expr<'a>(node: &'a MetaPropExpr, context: &mut Context<'a>) -> PrintItems {
    return parse_for_member_like_expr(MemberLikeExpr {
        left_node: (&node.meta).into(),
        right_node: (&node.prop).into(),
        is_computed: false,
    }, context);
}

fn parse_new_expr<'a>(node: &'a NewExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("new ");
    items.extend(parse_node((&node.callee).into(), context));
    if let Some(type_args) = &node.type_args { items.extend(parse_node(type_args.into(), context)); }
    let args = match node.args.as_ref() {
        Some(args) => args.iter().map(|node| node.into()).collect(),
        None => Vec::new(),
    };
    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: args,
        prefer_hanging: context.config.new_expression_prefer_hanging_arguments,
        custom_close_paren: None,
    }, context));
    return items;
}

fn parse_non_null_expr<'a>(node: &'a TsNonNullExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.expr).into(), context));
    items.push_str("!");
    return items;
}

fn parse_object_lit<'a>(node: &'a ObjectLit, context: &mut Context<'a>) -> PrintItems {
    return parse_object_like_node(ParseObjectLikeNodeOptions {
        node_span: node.span,
        members: node.props.iter().map(|x| x.into()).collect(),
        trailing_commas: Some(context.config.object_expression_trailing_commas),
    }, context);
}

fn parse_paren_expr<'a>(node: &'a ParenExpr, context: &mut Context<'a>) -> PrintItems {
    return conditions::with_indent_if_start_of_line_indented(parser_helpers::new_line_group(
        parse_node_in_parens(
            (&node.expr).into(),
            |context| parse_node((&node.expr).into(), context),
            context
        )
    )).into();
}

fn parse_sequence_expr<'a>(node: &'a SeqExpr, context: &mut Context<'a>) -> PrintItems {
    parse_comma_separated_values(node.exprs.iter().map(|x| x.into()).collect(), |_| { Some(false) }, context).items
}

fn parse_setter_prop<'a>(node: &'a SetterProp, context: &mut Context<'a>) -> PrintItems {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: None,
        accessibility: None,
        is_static: false,
        is_async: false,
        is_abstract: false,
        kind: ClassOrObjectMethodKind::Setter,
        is_generator: false,
        is_optional: false,
        key: (&node.key).into(),
        type_params: None,
        params: vec![(&node.param).into()],
        return_type: None,
        body: node.body.as_ref().map(|x| x.into()),
    }, context);
}

fn parse_spread_element<'a>(node: &'a SpreadElement, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("...");
    items.extend(parse_node((&node.expr).into(), context));
    return items;
}

fn parse_tagged_tpl<'a>(node: &'a TaggedTpl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.tag).into(), context));
    if let Some(type_params) = &node.type_params { items.extend(parse_node(type_params.into(), context)); }
    items.push_signal(Signal::SpaceOrNewLine);
    items.push_condition(conditions::indent_if_start_of_line(parse_template_literal(&node.quasis, &node.exprs.iter().map(|x| &**x).collect(), context)));
    return items;
}

fn parse_tpl<'a>(node: &'a Tpl, context: &mut Context<'a>) -> PrintItems {
    parse_template_literal(&node.quasis, &node.exprs.iter().map(|x| &**x).collect(), context)
}

fn parse_tpl_element<'a>(node: &'a TplElement, context: &mut Context<'a>) -> PrintItems {
    parse_raw_string(node.text(context).into())
}

fn parse_template_literal<'a>(quasis: &'a Vec<TplElement>, exprs: &Vec<&'a Expr>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("`");
    items.push_signal(Signal::StartIgnoringIndent);
    for node in get_nodes(quasis, exprs) {
        if node.kind() == NodeKind::TplElement {
            items.extend(parse_node(node, context));
        } else {
            items.push_str("${");
            items.push_signal(Signal::FinishIgnoringIndent);
            items.push_signal(Signal::PossibleNewLine);
            items.push_condition(conditions::single_indent_if_start_of_line());
            items.extend(parse_node(node, context));
            items.push_signal(Signal::PossibleNewLine);
            items.push_condition(conditions::single_indent_if_start_of_line());
            items.push_str("}");
            items.push_signal(Signal::StartIgnoringIndent);
        }
    }
    items.push_str("`");
    items.push_signal(Signal::FinishIgnoringIndent);
    return items;

    fn get_nodes<'a>(quasis: &'a Vec<TplElement>, exprs: &Vec<&'a Expr>) -> Vec<Node<'a>> {
        let quasis = quasis;
        let exprs = exprs;
        let mut nodes = Vec::new();
        let mut quasis_index = 0;
        let mut exprs_index = 0;

        while quasis_index < quasis.len() || exprs_index < exprs.len() {
            let current_quasis = quasis.get(quasis_index);
            let current_expr = exprs.get(exprs_index);

            let is_quasis = if let Some(current_quasis) = current_quasis {
                if let Some(current_expr) = current_expr {
                    if current_quasis.lo() < current_expr.lo() {
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }
            } else {
                false
            };

            if is_quasis {
                nodes.push((&quasis[quasis_index]).into());
                quasis_index += 1;
            } else {
                nodes.push(exprs[exprs_index].into());
                exprs_index += 1;
            }
        }

        return nodes;
    }
}

fn parse_type_assertion<'a>(node: &'a TsTypeAssertion, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("<");
    items.extend(parse_node((&node.type_ann).into(), context));
    items.push_str(">");
    if context.config.type_assertion_space_before_expression { items.push_str(" "); }
    items.extend(parse_node((&node.expr).into(), context));
    items
}

fn parse_unary_expr<'a>(node: &'a UnaryExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str(get_operator_text(node.op));
    items.extend(parse_node((&node.arg).into(), context));
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

fn parse_update_expr<'a>(node: &'a UpdateExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let operator_text = get_operator_text(node.op);
    if node.prefix {
        items.push_str(operator_text);
    }
    items.extend(parse_node((&node.arg).into(), context));
    if !node.prefix {
        items.push_str(operator_text);
    }
    return items;

    fn get_operator_text<'a>(operator: UpdateOp) -> &'a str {
        match operator {
            UpdateOp::MinusMinus => "--",
            UpdateOp::PlusPlus => "++",
        }
    }
}

fn parse_yield_expr<'a>(node: &'a YieldExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("yield");
    if node.delegate { items.push_str("*"); }
    if let Some(arg) = &node.arg {
        items.push_str(" ");
        items.extend(parse_node(arg.into(), context));
    }
    items
}

/* exports */

fn parse_export_named_specifier<'a>(node: &'a NamedExportSpecifier, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    items.extend(parse_node((&node.orig).into(), context));
    if let Some(exported) = &node.exported {
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str("as ");
            items.extend(parse_node(exported.into(), context));
            items
        }));
    }

    items
}

/* imports */

fn parse_import_named_specifier<'a>(node: &'a ImportSpecific, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    if let Some(imported) = &node.imported {
        items.extend(parse_node(imported.into(), context));
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str("as ");
            items.extend(parse_node((&node.local).into(), context));
            items
        }));
    } else {
        items.extend(parse_node((&node.local).into(), context));
    }

    items
}

fn parse_import_namespace_specifier<'a>(node: &'a ImportStarAs, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("* as ");
    items.extend(parse_node((&node.local).into(), context));
    return items;
}

fn parse_external_module_ref<'a>(node: &'a TsExternalModuleRef, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("require");
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&context.token_finder.get_first_open_paren_token_within(&node.span), &node.expr, context);
    items.extend(wrap_in_parens(parse_node((&node.expr).into(), context), use_new_lines));
    return items;
}

/* interface / type element */

fn parse_call_signature_decl<'a>(node: &'a TsCallSignatureDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let start_info = Info::new("startCallSignature");

    items.push_info(start_info);
    if let Some(type_params) = &node.type_params { items.extend(parse_node(type_params.into(), context)); }
    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.iter().map(|node| node.into()).collect(),
        prefer_hanging: context.config.call_signature_prefer_hanging_parameters,
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info,
            type_node: node.type_ann.as_ref().map(|x| x.into()),
            type_node_separator: None,
        }, context)),
    }, context));
    if context.config.call_signature_semi_colon { items.push_str(";"); }

    return items;
}

fn parse_construct_signature_decl<'a>(node: &'a TsConstructSignatureDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let start_info = Info::new("startConstructSignature");

    items.push_info(start_info);
    items.push_str("new");
    if context.config.construct_signature_space_after_new_keyword { items.push_str(" "); }
    if let Some(type_params) = &node.type_params { items.extend(parse_node(type_params.into(), context)); }
    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.iter().map(|node| node.into()).collect(),
        prefer_hanging: context.config.construct_signature_prefer_hanging_parameters,
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info,
            type_node: node.type_ann.as_ref().map(|x| x.into()),
            type_node_separator: None,
        }, context)),
    }, context));
    if context.config.construct_signature_semi_colon { items.push_str(";"); }

    return items;
}

fn parse_index_signature<'a>(node: &'a TsIndexSignature, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    if node.readonly { items.push_str("readonly "); }

    // todo: this should do something similar to the other declarations here (the ones with customCloseParen)
    items.push_str("[");
    items.extend(parse_node(node.params.iter().next().expect("Expected the index signature to have one parameter.").into(), context));
    items.push_str("]");
    items.extend(parse_type_annotation_with_colon_if_exists(&node.type_ann, context));
    if context.config.index_signature_semi_colon { items.push_str(";"); }

    return items;
}

fn parse_interface_body<'a>(node: &'a TsInterfaceBody, context: &mut Context<'a>) -> PrintItems {
    let start_header_info = get_parent_info(context);

    return parse_membered_body(ParseMemberedBodyOptions {
        span: node.span,
        members: node.body.iter().map(|x| x.into()).collect(),
        start_header_info: start_header_info,
        brace_position: context.config.interface_declaration_brace_position,
        should_use_blank_line: move |previous, next, context| {
            node_helpers::has_separating_blank_line(previous, next, context)
        },
        trailing_commas: None,
    }, context);

    fn get_parent_info(context: &mut Context) -> Option<Info> {
        for ancestor in context.parent_stack.iter() {
            if let Node::TsInterfaceDecl(ancestor) = ancestor {
                return context.get_info_for_node(&ancestor).map(|x| x.to_owned());
            }
        }
        return None;
    }
}

fn parse_method_signature<'a>(node: &'a TsMethodSignature, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let start_info = Info::new("startMethodSignature");
    items.push_info(start_info);

    if node.computed { items.push_str("["); }
    items.extend(parse_node((&node.key).into(), context));
    if node.computed { items.push_str("]"); }
    if node.optional { items.push_str("?"); }
    if let Some(type_params) = &node.type_params { items.extend(parse_node(type_params.into(), context)); }

    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.iter().map(|node| node.into()).collect(),
        prefer_hanging: context.config.method_signature_prefer_hanging_parameters,
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info,
            type_node: node.type_ann.as_ref().map(|x| x.into()),
            type_node_separator: None,
        }, context)),
    }, context));

    if context.config.method_signature_semi_colon { items.push_str(";"); }

    return items;
}

fn parse_property_signature<'a>(node: &'a TsPropertySignature, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if node.readonly { items.push_str("readonly "); }
    if node.computed { items.push_str("["); }
    items.extend(parse_node((&node.key).into(), context));
    if node.computed { items.push_str("]"); }
    if node.optional { items.push_str("?"); }
    items.extend(parse_type_annotation_with_colon_if_exists(&node.type_ann, context));

    if let Some(init) = &node.init {
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str("= ");
            items.extend(parse_node(init.into(), context));
            items
        }));
    }

    if context.config.property_signature_semi_colon { items.push_str(";"); }

    return items;
}

fn parse_type_lit<'a>(node: &'a TsTypeLit, context: &mut Context<'a>) -> PrintItems {
    return parse_object_like_node(ParseObjectLikeNodeOptions {
        node_span: node.span,
        members: node.members.iter().map(|m| m.into()).collect(),
        trailing_commas: None
    }, context);
}

/* jsx */

fn parse_jsx_attribute<'a>(node: &'a JSXAttr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.name).into(), context));
    if let Some(value) = &node.value {
        items.push_str("=");
        let surround_with_braces = context.token_finder.get_previous_token_if_open_brace(value).is_some();
        let parsed_value = parse_node(value.into(), context);
        items.extend(if surround_with_braces {
            parse_as_jsx_expr_container(parsed_value, context)
        } else {
            parsed_value
        });
    }
    return items;
}

fn parse_jsx_closing_element<'a>(node: &'a JSXClosingElement, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("</");
    items.extend(parse_node((&node.name).into(), context));
    items.push_str(">");
    return items;
}

fn parse_jsx_closing_fragment<'a>(_: &'a JSXClosingFragment, _: &mut Context<'a>) -> PrintItems {
    "</>".into()
}

fn parse_jsx_element<'a>(node: &'a JSXElement, context: &mut Context<'a>) -> PrintItems {
    if let Some(closing) = &node.closing {
        parse_jsx_with_opening_and_closing(ParseJsxWithOpeningAndClosingOptions {
            opening_element: (&node.opening).into(),
            closing_element: closing.into(),
            children: node.children.iter().map(|x| x.into()).collect(),
        }, context)
    } else {
        parse_node((&node.opening).into(), context)
    }
}

fn parse_jsx_empty_expr<'a>(node: &'a JSXEmptyExpr, context: &mut Context<'a>) -> PrintItems {
    parse_comment_collection(get_jsx_empty_expr_comments(node, context), None, context)
}

fn parse_jsx_expr_container<'a>(node: &'a JSXExprContainer, context: &mut Context<'a>) -> PrintItems {
    // Don't send JSX empty expressions to parse_node because it will not handle comments
    // the way they should be specifically handled for empty expressions.
    let expr_items = match &node.expr {
        JSXExpr::JSXEmptyExpr(expr) => parse_jsx_empty_expr(expr, context),
        JSXExpr::Expr(expr) => parse_node(expr.into(), context),
    };

    parse_as_jsx_expr_container(expr_items, context)
}

fn parse_as_jsx_expr_container(parsed_node: PrintItems, context: &mut Context) -> PrintItems {
    let surround_with_space = context.config.jsx_expression_container_space_surrounding_expression;
    let mut items = PrintItems::new();

    items.push_str("{");
    if surround_with_space { items.push_str(" "); }
    items.extend(parsed_node);
    if surround_with_space { items.push_str(" "); }
    items.push_str("}");

    return items;
}

fn parse_jsx_fragment<'a>(node: &'a JSXFragment, context: &mut Context<'a>) -> PrintItems {
    parse_jsx_with_opening_and_closing(ParseJsxWithOpeningAndClosingOptions {
        opening_element: (&node.opening).into(),
        closing_element: (&node.closing).into(),
        children: node.children.iter().map(|x| x.into()).collect(),
    }, context)
}

fn parse_jsx_member_expr<'a>(node: &'a JSXMemberExpr, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.obj).into(), context));
    items.push_str(".");
    items.extend(parse_node((&node.prop).into(), context));
    return items;
}

fn parse_jsx_namespaced_name<'a>(node: &'a JSXNamespacedName, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.ns).into(), context));
    items.push_str(":");
    items.extend(parse_node((&node.name).into(), context));
    return items;
}

fn parse_jsx_opening_element<'a>(node: &'a JSXOpeningElement, context: &mut Context<'a>) -> PrintItems {
    let is_multi_line = get_is_multi_line(node, context);
    let start_info = Info::new("openingElementStartInfo");
    let mut items = PrintItems::new();

    items.push_info(start_info);
    items.push_str("<");
    items.extend(parse_node((&node.name).into(), context));
    if let Some(type_args) = &node.type_args {
        items.extend(parse_node(type_args.into(), context));
    }
    items.extend(parse_attribs(&node.attrs, is_multi_line, context));
    if node.self_closing {
        if !is_multi_line {
            items.push_str(" ");
        }
        items.push_str("/");
    } else {
        items.push_condition(conditions::new_line_if_hanging(start_info, None));
    }
    items.push_str(">");

    return items;

    fn parse_attribs<'a>(attribs: &'a Vec<JSXAttrOrSpread>, is_multi_line: bool, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        if attribs.is_empty() {
            return items;
        }

        for attrib in attribs {
            items.push_signal(if is_multi_line {
                Signal::NewLine
            } else {
                Signal::SpaceOrNewLine
            });

            items.push_condition(conditions::indent_if_start_of_line({
                match attrib {
                    JSXAttrOrSpread::JSXAttr(attr) => parse_node(attr.into(), context),
                    JSXAttrOrSpread::SpreadElement(element) => {
                        parse_as_jsx_expr_container(parse_node(element.into(), context), context)
                    }
                }
            }));
        }

        if is_multi_line {
            items.push_signal(Signal::NewLine);
        }

        return items;
    }

    fn get_is_multi_line(node: &JSXOpeningElement, context: &mut Context) -> bool {
        if let Some(first_attrib) = node.attrs.first() {
            node_helpers::get_use_new_lines_for_nodes(&node.name, first_attrib, context)
        } else {
            false
        }
    }
}

fn parse_jsx_opening_fragment<'a>(_: &'a JSXOpeningFragment, _: &mut Context<'a>) -> PrintItems {
    "<>".into()
}

fn parse_jsx_spread_child<'a>(node: &'a JSXSpreadChild, context: &mut Context<'a>) -> PrintItems {
    parse_as_jsx_expr_container({
        let mut items = PrintItems::new();
        items.push_str("...");
        items.extend(parse_node((&node.expr).into(), context));
        items
    }, context)
}

fn parse_jsx_text<'a>(node: &'a JSXText, context: &mut Context<'a>) -> PrintItems {
    let lines = node.text(context).trim().lines().map(|line| line.trim_end());
    let mut past_line: Option<&str> = None;
    let mut past_past_line: Option<&str> = None;
    let mut items = PrintItems::new();

    for line in lines {
        if let Some(past_line) = past_line {
            if !line.is_empty() || past_past_line.is_none() {
                items.push_signal(Signal::NewLine);
            } else if let Some(past_past_line) = past_past_line {
                if past_line.is_empty() && !past_past_line.is_empty() {
                    items.push_signal(Signal::NewLine);
                }
            }
        }

        if !line.is_empty() {
            items.push_str(line);
        }

        past_past_line = std::mem::replace(&mut past_line, Some(line));
    }

    return items;
}

/* literals */

fn parse_big_int_literal<'a>(node: &'a BigInt, context: &mut Context<'a>) -> PrintItems {
    node.text(context).into()
}

fn parse_bool_literal(node: &Bool) -> PrintItems {
    match node.value {
        true => "true",
        false => "false",
    }.into()
}

fn parse_num_literal<'a>(node: &'a Number, context: &mut Context<'a>) -> PrintItems {
    node.text(context).into()
}

fn parse_reg_exp_literal(node: &Regex, _: &mut Context) -> PrintItems {
    // the exp and flags should not be nodes so just ignore that (swc issue #511)
    let mut items = PrintItems::new();
    items.push_str("/");
    items.push_str(&node.exp as &str);
    items.push_str("/");
    items.push_str(&node.flags as &str);
    items
}

fn parse_string_literal<'a>(node: &'a Str, context: &mut Context<'a>) -> PrintItems {
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

fn parse_module<'a>(node: &'a Module, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if let Some(shebang) = &node.shebang {
        items.push_str("#!");
        items.push_str(&shebang as &str);
        items.push_signal(Signal::NewLine);
        if let Some(first_statement) = node.body.first() {
            if node_helpers::has_separating_blank_line(&node.span.lo(), &first_statement, context) {
                items.push_signal(Signal::NewLine);
            }
        }
    }
    items.extend(parse_statements_or_members(ParseStatementsOrMembersOptions {
        inner_span: node.span,
        items: node.body.iter().map(|module_item| (module_item.into(), None)).collect(),
        should_use_space: None,
        should_use_new_line: None,
        should_use_blank_line: |previous, next, context| node_helpers::has_separating_blank_line(previous, next, context),
        trailing_commas: None,
    }, context));
    return items;
}

/* patterns */

fn parse_array_pat<'a>(node: &'a ArrayPat, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_array_like_nodes(ParseArrayLikeNodesOptions {
        parent_span: node.span,
        elements: node.elems.iter().map(|x| x.as_ref().map(|elem| elem.into())).collect(),
        trailing_commas: context.config.array_pattern_trailing_commas,
    }, context));
    items.extend(parse_type_annotation_with_colon_if_exists(&node.type_ann, context));
    items
}

fn parse_assign_pat<'a>(node: &'a AssignPat, context: &mut Context<'a>) -> PrintItems {
    parser_helpers::new_line_group({
        let mut items = PrintItems::new();
        items.extend(parse_node((&node.left).into(), context));
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str("= ");
            items.extend(parse_node((&node.right).into(), context));
            items
        }));
        items
    })
}

fn parse_assign_pat_prop<'a>(node: &'a AssignPatProp, context: &mut Context<'a>) -> PrintItems {
    return parser_helpers::new_line_group({
        let mut items = PrintItems::new();
        items.extend(parse_node((&node.key).into(), context));
        if let Some(value) = &node.value {
            items.push_signal(Signal::SpaceOrNewLine);
            items.push_condition(conditions::indent_if_start_of_line({
                let mut items = PrintItems::new();
                items.push_str("= ");
                items.extend(parse_node(value.into(), context));
                items
            }));
        }
        items
    });
}

fn parse_rest_pat<'a>(node: &'a RestPat, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("...");
    items.extend(parse_node((&node.arg).into(), context));
    items.extend(parse_type_annotation_with_colon_if_exists(&node.type_ann, context));
    items
}

fn parse_object_pat<'a>(node: &'a ObjectPat, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_object_like_node(ParseObjectLikeNodeOptions {
        node_span: node.span,
        members: node.props.iter().map(|x| x.into()).collect(),
        trailing_commas: Some(TrailingCommas::Never),
    }, context));
    items.extend(parse_type_annotation_with_colon_if_exists(&node.type_ann, context));
    return items;
}

/* properties */

fn parse_method_prop<'a>(node: &'a MethodProp, context: &mut Context<'a>) -> PrintItems {
    return parse_class_or_object_method(ClassOrObjectMethod {
        decorators: None,
        accessibility: None,
        is_static: false,
        is_async: node.function.is_async,
        is_abstract: false,
        kind: ClassOrObjectMethodKind::Method,
        is_generator: node.function.is_generator,
        is_optional: false,
        key: (&node.key).into(),
        type_params: node.function.type_params.as_ref().map(|x| x.into()),
        params: node.function.params.iter().map(|x| x.into()).collect(),
        return_type: node.function.return_type.as_ref().map(|x| x.into()),
        body: node.function.body.as_ref().map(|x| x.into()),
    }, context);
}

struct ClassOrObjectMethod<'a> {
    decorators: Option<&'a Vec<Decorator>>,
    accessibility: Option<Accessibility>,
    is_static: bool,
    is_async: bool,
    is_abstract: bool,
    kind: ClassOrObjectMethodKind,
    is_generator: bool,
    is_optional: bool,
    key: Node<'a>,
    type_params: Option<Node<'a>>,
    params: Vec<Node<'a>>,
    return_type: Option<Node<'a>>,
    body: Option<Node<'a>>,
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

fn parse_class_or_object_method<'a>(node: ClassOrObjectMethod<'a>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if let Some(decorators) = node.decorators.as_ref() {
        items.extend(parse_decorators(decorators, false, context));
    }

    let start_header_info = Info::new("methodStartHeaderInfo");
    items.push_info(start_header_info);

    if let Some(accessibility) = node.accessibility {
        items.push_str(&format!("{} ", accessibility_to_str(&accessibility)));
    }
    if node.is_static { items.push_str("static "); }
    if node.is_async { items.push_str("async "); }
    if node.is_abstract { items.push_str("abstract "); }

    match node.kind {
        ClassOrObjectMethodKind::Getter => items.push_str("get "),
        ClassOrObjectMethodKind::Setter => items.push_str("set "),
        ClassOrObjectMethodKind::Method | ClassOrObjectMethodKind::Constructor => {},
    }

    if node.is_generator { items.push_str("*"); }
    items.extend(parse_node(node.key, context));
    if node.is_optional { items.push_str("?"); }
    if let Some(type_params) = node.type_params { items.extend(parse_node(type_params, context)); }
    if get_use_space_before_parens(&node.kind, context) { items.push_str(" ") }

    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.into_iter().map(|node| node.into()).collect(),
        prefer_hanging: get_prefer_hanging_parameters(&node.kind, context),
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info: start_header_info,
            type_node: node.return_type,
            type_node_separator: None,
        }, context)),
    }, context));

    if let Some(body) = node.body {
        let brace_position = get_brace_position(&node.kind, context);
        items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
            brace_position: brace_position,
            open_brace_token: context.token_finder.get_first_open_brace_token_within(&body),
            start_header_info: Some(start_header_info),
        }, context));
        items.extend(parse_node(body, context));
    } else if get_use_semi_colon(&node.kind, context) {
        items.push_str(";");
    }

    return items;

    fn get_prefer_hanging_parameters(kind: &ClassOrObjectMethodKind, context: &mut Context) -> bool {
        match kind {
            ClassOrObjectMethodKind::Constructor => context.config.constructor_prefer_hanging_parameters,
            ClassOrObjectMethodKind::Getter => context.config.get_accessor_prefer_hanging_parameters,
            ClassOrObjectMethodKind::Setter => context.config.set_accessor_prefer_hanging_parameters,
            ClassOrObjectMethodKind::Method => context.config.method_prefer_hanging_parameters,
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

fn parse_block_stmt<'a>(node: &'a BlockStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let start_statements_info = Info::new("startStatementsInfo");
    let end_statements_info = Info::new("endStatementsInfo");
    let open_brace_token = context.token_finder.get_first_open_brace_token_within(&node);

    items.push_str("{");
    let after_open_brace_info = Info::new("after_open_brace_info");
    let open_brace_trailing_comments = open_brace_token.trailing_comments(context);
    let open_brace_trailing_comments_ends_with_comment_block = open_brace_trailing_comments.get_last_comment().map(|x| x.kind == CommentKind::Block).unwrap_or(false);
    let is_braces_same_line_and_empty = node.start_line(context) == node.end_line(context) && node.stmts.is_empty();
    items.extend(parse_comments_as_trailing(&open_brace_token, open_brace_trailing_comments, context));
    items.extend(parse_first_line_trailing_comments(&node, node.stmts.get(0).map(|x| x as &dyn Spanned), context));

    if !is_braces_same_line_and_empty {
        items.push_signal(Signal::NewLine);
    }
    items.push_info(start_statements_info);
    items.extend(parser_helpers::with_indent(
        parse_statements(node.get_inner_span(context), node.stmts.iter().map(|stmt| stmt.into()).collect(), context)
    ));
    items.push_info(end_statements_info);

    if is_braces_same_line_and_empty {
        items.push_condition(if_true_or(
            "newLineIfDifferentLine",
            move |context| condition_resolvers::is_on_different_line(context, &after_open_brace_info),
            Signal::NewLine.into(),
            {
                if open_brace_trailing_comments_ends_with_comment_block {
                    Signal::SpaceOrNewLine.into()
                } else {
                    PrintItems::new()
                }
            }
        ));
    } else {
        items.push_condition(Condition::new("endStatementsNewLine", ConditionProperties {
            condition: Box::new(move |context| {
                condition_resolvers::are_infos_equal(context, &start_statements_info, &end_statements_info)
            }),
            true_path: None,
            false_path: Some(Signal::NewLine.into()),
        }));
    }

    items.push_str("}");

    return items;
}

fn parse_break_stmt<'a>(node: &'a BreakStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    items.push_str("break");
    if let Some(label) = &node.label {
        items.push_str(" ");
        items.extend(parse_node(label.into(), context));
    }
    if context.config.break_statement_semi_colon {
        items.push_str(";");
    }

    items
}

fn parse_continue_stmt<'a>(node: &'a ContinueStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    items.push_str("continue");
    if let Some(label) = &node.label {
        items.push_str(" ");
        items.extend(parse_node(label.into(), context));
    }
    if context.config.continue_statement_semi_colon {
        items.push_str(";");
    }

    items
}

fn parse_debugger_stmt<'a>(_: &'a DebuggerStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    items.push_str("debugger");
    if context.config.debugger_statement_semi_colon {
        items.push_str(";");
    }

    items
}

fn parse_do_while_stmt<'a>(node: &'a DoWhileStmt, context: &mut Context<'a>) -> PrintItems {
    // the braces are technically optional on do while statements
    let mut items = PrintItems::new();
    items.push_str("do");
    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: context.config.do_while_statement_brace_position,
        open_brace_token: if let Stmt::Block(_) = &*node.body { context.token_finder.get_first_open_brace_token_within(&node) } else { None },
        start_header_info: None,
    }, context));
    items.extend(parse_node((&node.body).into(), context));
    items.push_str(" while");
    if context.config.do_while_statement_space_after_while_keyword {
        items.push_str(" ");
    }
    items.extend(parse_node_in_parens((&node.test).into(), |context| parse_node((&node.test).into(), context), context));
    if context.config.do_while_statement_semi_colon {
        items.push_str(";");
    }
    return items;
}

fn parse_export_all<'a>(node: &'a ExportAll, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("export * from ");
    items.extend(parse_node((&node.src).into(), context));

    if context.config.export_all_declaration_semi_colon {
        items.push_str(";");
    }

    items
}

fn parse_empty_stmt(_: &EmptyStmt, _: &mut Context) -> PrintItems {
    ";".into()
}

fn parse_export_assignment<'a>(node: &'a TsExportAssignment, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    items.push_str("export = ");
    items.extend(parse_node((&node.expr).into(), context));
    if context.config.export_assignment_semi_colon {
        items.push_str(";");
    }

    items
}

fn parse_namespace_export<'a>(node: &'a TsNamespaceExportDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("export as namespace ");
    items.extend(parse_node((&node.id).into(), context));

    if context.config.namespace_export_declaration_semi_colon {
        items.push_str(";");
    }

    items
}

fn parse_expr_stmt<'a>(stmt: &'a ExprStmt, context: &mut Context<'a>) -> PrintItems {
    if context.config.expression_statement_semi_colon {
        return parse_inner(&stmt, context);
    } else {
        return parse_for_prefix_semi_colon_insertion(&stmt, context);
    }

    fn parse_inner<'a>(stmt: &'a ExprStmt, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        items.extend(parse_node((&stmt.expr).into(), context));
        if context.config.expression_statement_semi_colon {
            items.push_str(";");
        }
        return items;
    }

    fn parse_for_prefix_semi_colon_insertion<'a>(stmt: &'a ExprStmt, context: &mut Context<'a>) -> PrintItems {
        let parsed_node = parse_inner(&stmt, context);
        let parsed_node = parsed_node.into_rc_path();
        return if should_add_semi_colon(&parsed_node).unwrap_or(false) {
            let mut items = PrintItems::new();
            items.push_str(";");
            items.extend(parsed_node.into());
            items
        } else {
            parsed_node.into()
        };

        fn should_add_semi_colon(path: &Option<PrintItemPath>) -> Option<bool> {
            // todo: this needs to be improved
            if let Some(path) = path {
                for item in PrintItemsIterator::new(path.clone()) {
                    match item {
                        PrintItem::String(value) => {
                            if let Some(c) = value.text.chars().next() {
                                return utils::is_prefix_semi_colon_insertion_char(c).into();
                            }
                        },
                        PrintItem::Condition(condition) => {
                            // It's an assumption here that the true and false paths of the
                            // condition will both contain the same text to look for. This is probably not robust
                            // and perhaps instead there should be a way to do something like "get the next character" in
                            // the printer.
                            if let Some(result) = should_add_semi_colon(&condition.get_true_path()) {
                                return Some(result);
                            }
                            if let Some(result) = should_add_semi_colon(&condition.get_false_path()) {
                                return Some(result);
                            }
                        },
                        PrintItem::RcPath(items) => {
                            if let Some(result) = should_add_semi_colon(&Some(items)) {
                                return Some(result);
                            }
                        },
                        _ => { /* do nothing */ },
                    }
                }
            }

            None
        }
    }
}

fn parse_for_stmt<'a>(node: &'a ForStmt, context: &mut Context<'a>) -> PrintItems {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = PrintItems::new();
    items.push_info(start_header_info);
    items.push_str("for");
    if context.config.for_statement_space_after_for_keyword {
        items.push_str(" ");
    }
    items.extend(parse_node_in_parens({
        if let Some(init) = &node.init {
            init.into()
        } else {
            context.token_finder.get_first_semi_colon_within(&node).expect("Expected to find a semi-colon within the for stmt.").into()
        }
    }, |context| {
        let mut items = PrintItems::new();
        let separator_after_semi_colons = if context.config.for_statement_space_after_semi_colons { Signal::SpaceOrNewLine } else { Signal::PossibleNewLine };
        items.extend(parser_helpers::new_line_group({
            let mut items = PrintItems::new();
            if let Some(init) = &node.init {
                items.extend(parse_node(init.into(), context));
            }
            items.push_str(";");
            items
        }));
        items.push_signal(separator_after_semi_colons);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            if let Some(test) = &node.test {
                items.extend(parse_node(test.into(), context));
            }
            items.push_str(";");
            items
        }));
        items.push_signal(separator_after_semi_colons);
        if let Some(update) = &node.update {
            items.push_condition(conditions::indent_if_start_of_line(parse_node(update.into(), context)));
        }
        items
    }, context));
    items.push_info(end_header_info);

    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: (&node.body).into(),
        use_braces: context.config.for_statement_use_braces,
        brace_position: context.config.for_statement_brace_position,
        single_body_position: Some(context.config.for_statement_single_body_position),
        requires_braces_condition_ref: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);

    return items;
}

fn parse_for_in_stmt<'a>(node: &'a ForInStmt, context: &mut Context<'a>) -> PrintItems {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = PrintItems::new();
    items.push_info(start_header_info);
    items.push_str("for");
    if context.config.for_in_statement_space_after_for_keyword {
        items.push_str(" ");
    }
    items.extend(parse_node_in_parens((&node.left).into(), |context| {
        let mut items = PrintItems::new();
        items.extend(parse_node((&node.left).into(), context));
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str("in ");
            items.extend(parse_node((&node.right).into(), context));
            items
        }));
        items
    }, context));
    items.push_info(end_header_info);

    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: (&node.body).into(),
        use_braces: context.config.for_in_statement_use_braces,
        brace_position: context.config.for_in_statement_brace_position,
        single_body_position: Some(context.config.for_in_statement_single_body_position),
        requires_braces_condition_ref: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);

    return items;
}

fn parse_for_of_stmt<'a>(node: &'a ForOfStmt, context: &mut Context<'a>) -> PrintItems {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = PrintItems::new();
    items.push_info(start_header_info);
    items.push_str("for");
    if context.config.for_of_statement_space_after_for_keyword {
        items.push_str(" ");
    }
    if let Some(await_token) = &node.await_token {
        items.extend(parse_node(await_token.into(), context));
        items.push_str(" ");
    }
    items.extend(parse_node_in_parens((&node.left).into(), |context| {
        let mut items = PrintItems::new();
        items.extend(parse_node((&node.left).into(), context));
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str("of ");
            items.extend(parse_node((&node.right).into(), context));
            items
        }));
        items
    }, context));
    items.push_info(end_header_info);

    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: (&node.body).into(),
        use_braces: context.config.for_of_statement_use_braces,
        brace_position: context.config.for_of_statement_brace_position,
        single_body_position: Some(context.config.for_of_statement_single_body_position),
        requires_braces_condition_ref: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);

    return items;
}

fn parse_if_stmt<'a>(node: &'a IfStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let cons = &*node.cons;
    let cons_span = cons.span();
    let result = parse_header_with_conditional_brace_body(ParseHeaderWithConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: cons.into(),
        parsed_header: {
            let mut items = PrintItems::new();
            items.push_str("if");
            if context.config.if_statement_space_after_if_keyword { items.push_str(" "); }
            let test = &*node.test;
            items.extend(parse_node_in_parens(test.into(), |context| parse_node(test.into(), context), context));
            items
        },
        use_braces: context.config.if_statement_use_braces,
        brace_position: context.config.if_statement_brace_position,
        single_body_position: Some(context.config.if_statement_single_body_position),
        requires_braces_condition_ref: context.take_if_stmt_last_brace_condition_ref(),
    }, context);

    items.extend(result.parsed_node);

    if let Some(alt) = &node.alt {
        if let Stmt::If(alt_alt) = &**alt {
            if alt_alt.alt.is_none() {
                context.store_if_stmt_last_brace_condition_ref(result.open_brace_condition_ref);
            }
        }

        items.extend(parse_control_flow_separator(context.config.if_statement_next_control_flow_position, &cons_span, "else", context));

        // parse the leading comments before the else keyword
        let else_keyword = context.token_finder.get_first_else_keyword_within(&Span::new(cons_span.hi(), alt.lo(), Default::default())).expect("Expected to find an else keyword.");
        items.extend(parse_leading_comments(else_keyword, context));
        items.extend(parse_leading_comments(&alt, context));

        let start_else_header_info = Info::new("startElseHeader");
        items.push_info(start_else_header_info);
        items.push_str("else");

        if let Stmt::If(alt) = &**alt {
            items.push_str(" ");
            items.extend(parse_node(alt.into(), context));
        } else {
            items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
                parent: &node.span,
                body_node: alt.into(),
                use_braces: context.config.if_statement_use_braces,
                brace_position: context.config.if_statement_brace_position,
                single_body_position: Some(context.config.if_statement_single_body_position),
                requires_braces_condition_ref: Some(result.open_brace_condition_ref),
                header_start_token: Some(else_keyword),
                start_header_info: Some(start_else_header_info),
                end_header_info: None,
            }, context).parsed_node);
        }
    }

    return items;
}

fn parse_labeled_stmt<'a>(node: &'a LabeledStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.label).into(), context));
    items.push_str(":");

    // not bothering to make this configurable, because who uses labeled statements?
    if node.body.kind() == NodeKind::BlockStmt {
        items.push_str(" ");
    } else {
        items.push_signal(Signal::NewLine);
    }

    items.extend(parse_node((&node.body).into(), context));

    return items;
}

fn parse_return_stmt<'a>(node: &'a ReturnStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("return");
    if let Some(arg) = &node.arg {
        items.push_str(" ");
        items.extend(parse_node(arg.into(), context));
    }
    if context.config.return_statement_semi_colon { items.push_str(";"); }
    return items;
}

fn parse_switch_stmt<'a>(node: &'a SwitchStmt, context: &mut Context<'a>) -> PrintItems {
    let start_header_info = Info::new("startHeader");
    let mut items = PrintItems::new();
    items.push_info(start_header_info);
    items.push_str("switch ");
    items.extend(parse_node_in_parens((&node.discriminant).into(), |context| parse_node((&node.discriminant).into(), context), context));
    items.extend(parse_membered_body(ParseMemberedBodyOptions {
        span: node.span,
        members: node.cases.iter().map(|x| x.into()).collect(),
        start_header_info: Some(start_header_info),
        brace_position: context.config.switch_statement_brace_position,
        should_use_blank_line: |_, _, _| false,
        trailing_commas: None,
    }, context));
    return items;
}

fn parse_switch_case<'a>(node: &'a SwitchCase, context: &mut Context<'a>) -> PrintItems {
    let block_stmt_body = get_block_stmt_body(&node);
    let start_header_info = Info::new("switchCaseStartHeader");
    let mut items = PrintItems::new();
    let colon_token = context.token_finder.get_first_colon_token_after(&if let Some(test) = &node.test {
        test.span().hi()
    } else {
        node.span.lo()
    }).expect("Expected to find a colon token.");

    items.push_info(start_header_info);

    if let Some(test) = &node.test {
        items.push_str("case ");
        items.extend(parse_node(test.into(), context));
        items.push_str(":");
    } else {
        items.push_str("default:");
    }

    items.extend(parse_first_line_trailing_comments(&node.span, node.cons.get(0).map(|x| x as &dyn Spanned), context));
    let parsed_trailing_comments = parse_trailing_comments_for_case(node.span, &block_stmt_body, context);
    if !node.cons.is_empty() {
        if let Some(block_stmt_body) = block_stmt_body {
            items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
                brace_position: context.config.switch_case_brace_position,
                open_brace_token: context.token_finder.get_first_open_brace_token_within(&block_stmt_body),
                start_header_info: None,
            }, context));
            items.extend(parse_node(node.cons.iter().next().unwrap().into(), context));
        } else {
            items.push_signal(Signal::NewLine);
            items.extend(parser_helpers::with_indent(parse_statements_or_members(ParseStatementsOrMembersOptions {
                inner_span: Span::new(colon_token.hi(), node.span.hi(), Default::default()),
                items: node.cons.iter().map(|node| (node.into(), None)).collect(),
                should_use_space: None,
                should_use_new_line: None,
                should_use_blank_line: |previous, next, context| node_helpers::has_separating_blank_line(previous, next, context),
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

    fn parse_trailing_comments_for_case<'a>(node_span: Span, block_stmt_body: &Option<Span>, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
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

fn parse_throw_stmt<'a>(node: &'a ThrowStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("throw ");
    items.extend(parse_node((&node.arg).into(), context));
    if context.config.throw_statement_semi_colon { items.push_str(";"); }
    return items;
}

fn parse_try_stmt<'a>(node: &'a TryStmt, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let brace_position = context.config.try_statement_brace_position;
    let next_control_flow_position = context.config.try_statement_next_control_flow_position;
    let mut last_block_span = node.block.span;

    items.push_str("try");
    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: brace_position,
        open_brace_token: context.token_finder.get_first_open_brace_token_within(&node.block),
        start_header_info: None,
    }, context));
    items.extend(parse_node((&node.block).into(), context));

    if let Some(handler) = &node.handler {
        items.extend(parse_control_flow_separator(next_control_flow_position, &last_block_span, "catch", context));
        last_block_span = handler.span;
        items.extend(parse_node(handler.into(), context));
    }

    if let Some(finalizer) = &node.finalizer {
        items.extend(parse_control_flow_separator(next_control_flow_position, &last_block_span, "finally", context));
        items.push_str("finally");
        items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
            brace_position: brace_position,
            open_brace_token: context.token_finder.get_first_open_brace_token_within(&finalizer),
            start_header_info: None,
        }, context));
        items.extend(parse_node(finalizer.into(), context));
    }

    return items;
}

fn parse_var_decl<'a>(node: &'a VarDecl, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if node.declare { items.push_str("declare "); }
    items.push_str(match node.kind {
        VarDeclKind::Const => "const ",
        VarDeclKind::Let => "let ",
        VarDeclKind::Var => "var ",
    });

    for (i, decl) in node.decls.iter().enumerate() {
        if i > 0 {
            items.push_str(",");
            items.push_signal(Signal::SpaceOrNewLine);
        }

        items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node(decl.into(), context))));
    }

    if requires_semi_colon(&node.span, context) { items.push_str(";"); }

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

fn parse_var_declarator<'a>(node: &'a VarDeclarator, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    items.extend(parse_node((&node.name).into(), context));

    if let Some(init) = &node.init {
        items.push_str(" =");
        items.extend(Signal::SpaceOrNewLine.into());
        items.push_condition(conditions::indent_if_start_of_line(parse_node(init.into(), context)));
    }

    items
}

fn parse_while_stmt<'a>(node: &'a WhileStmt, context: &mut Context<'a>) -> PrintItems {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = PrintItems::new();
    items.push_info(start_header_info);
    items.push_str("while");
    if context.config.while_statement_space_after_while_keyword {
        items.push_str(" ");
    }
    items.extend(parse_node_in_parens((&node.test).into(), |context| parse_node((&node.test).into(), context), context));
    items.push_info(end_header_info);
    items.extend(parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: &node.span,
        body_node: (&node.body).into(),
        use_braces: context.config.while_statement_use_braces,
        brace_position: context.config.while_statement_brace_position,
        single_body_position: Some(context.config.while_statement_single_body_position),
        requires_braces_condition_ref: None,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context).parsed_node);
    return items;
}

/* types */

fn parse_array_type<'a>(node: &'a TsArrayType, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.elem_type).into(), context));
    items.push_str("[]");
    return items;
}

fn parse_conditional_type<'a>(node: &'a TsConditionalType, context: &mut Context<'a>) -> PrintItems {
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&node.check_type, &node.false_type, context);
    let is_parent_conditional_type = context.parent().kind() == NodeKind::TsConditionalType;
    let mut items = PrintItems::new();

    // main area
    items.extend(parser_helpers::new_line_group(parse_node((&node.check_type).into(), context)));
    items.push_signal(Signal::SpaceOrNewLine);
    items.push_condition(conditions::indent_if_start_of_line({
        let mut items = PrintItems::new();
        items.push_str("extends ");
        items.extend(parser_helpers::new_line_group(parse_node((&node.extends_type).into(), context)));
        items
    }));
    items.push_signal(Signal::SpaceOrNewLine);
    items.push_condition(conditions::indent_if_start_of_line({
        let mut items = PrintItems::new();
        items.push_str("? ");
        items.extend(parser_helpers::new_line_group(parse_node((&node.true_type).into(), context)));
        items
    }));

    // false type
    items.push_signal(if use_new_lines { Signal::NewLine } else { Signal::SpaceOrNewLine });

    let false_type_parsed = {
        let mut items = PrintItems::new();
        items.push_str(": ");
        items.extend(parser_helpers::new_line_group(parse_node((&node.false_type).into(), context)));
        items
    };

    if is_parent_conditional_type {
        items.extend(false_type_parsed);
    } else {
        items.push_condition(conditions::indent_if_start_of_line(false_type_parsed));
    }

    return items;
}

fn parse_constructor_type<'a>(node: &'a TsConstructorType, context: &mut Context<'a>) -> PrintItems {
    let start_info = Info::new("startConstructorType");
    let mut items = PrintItems::new();
    items.push_info(start_info);
    items.push_str("new");
    if context.config.constructor_type_space_after_new_keyword { items.push_str(" "); }
    if let Some(type_params) = &node.type_params {
        items.extend(parse_node(type_params.into(), context));
    }
    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.iter().map(|node| node.into()).collect(),
        prefer_hanging: context.config.constructor_type_prefer_hanging_parameters,
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info,
            type_node: Some((&node.type_ann).into()),
            type_node_separator: Some({
                let mut items = PrintItems::new();
                items.push_signal(Signal::SpaceOrNewLine);
                items.push_str("=> ");
                items
            }),
        }, context)),
    }, context));
    return items;
}

fn parse_function_type<'a>(node: &'a TsFnType, context: &mut Context<'a>) -> PrintItems {
    let start_info = Info::new("startFunctionType");
    let mut items = PrintItems::new();
    items.push_info(start_info);
    if let Some(type_params) = &node.type_params {
        items.extend(parse_node(type_params.into(), context));
    }
    items.extend(parse_parameters_or_arguments(ParseParametersOrArgumentsOptions {
        nodes: node.params.iter().map(|node| node.into()).collect(),
        prefer_hanging: context.config.function_type_prefer_hanging_parameters,
        custom_close_paren: Some(parse_close_paren_with_type(ParseCloseParenWithTypeOptions {
            start_info,
            type_node: Some((&node.type_ann).into()),
            type_node_separator: {
                let mut items = PrintItems::new();
                items.push_signal(Signal::SpaceOrNewLine);
                items.push_str("=> ");
                Some(items)
            },
        }, context)),
    }, context));
    return items;
}

fn parse_import_type<'a>(node: &'a TsImportType, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("import(");
    items.extend(parse_node((&node.arg).into(), context));
    items.push_str(")");

    if let Some(qualifier) = &node.qualifier {
        items.push_str(".");
        items.extend(parse_node(qualifier.into(), context));
    }

    if let Some(type_args) = &node.type_args {
        items.extend(parse_node(type_args.into(), context));
    }
    return items;
}

fn parse_indexed_access_type<'a>(node: &'a TsIndexedAccessType, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.obj_type).into(), context));
    items.push_str("[");
    items.extend(parse_node((&node.index_type).into(), context));
    items.push_str("]");
    return items;
}

fn parse_infer_type<'a>(node: &'a TsInferType, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("infer ");
    items.extend(parse_node((&node.type_param).into(), context));
    return items;
}

fn parse_intersection_type<'a>(node: &'a TsIntersectionType, context: &mut Context<'a>) -> PrintItems {
    parse_union_or_intersection_type(UnionOrIntersectionType {
        span: node.span,
        types: &node.types,
        is_union: false,
    }, context)
}

fn parse_lit_type<'a>(node: &'a TsLitType, context: &mut Context<'a>) -> PrintItems {
    parse_node((&node.lit).into(), context)
}

fn parse_mapped_type<'a>(node: &'a TsMappedType, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let start_info = Info::new("startMappedType");
    let end_info = Info::new("endMappedType");
    let open_brace_token = context.token_finder.get_first_open_brace_token_within(&node).expect("Expected to find an open brace token in the mapped type.");
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&open_brace_token, &node.type_param, context);
    items.push_info(start_info);
    items.push_str("{");
    if use_new_lines {
        items.push_signal(Signal::NewLine);
    } else {
        items.push_condition(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_info, Some(end_info)));
    }
    items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group({
        let mut items = PrintItems::new();
        if let Some(readonly) = node.readonly {
            items.push_str(match readonly {
                TruePlusMinus::True => "readonly ",
                TruePlusMinus::Plus => "+readonly ",
                TruePlusMinus::Minus => "-readonly ",
            });
        }
        items.push_str("[");
        items.extend(parse_node((&node.type_param).into(), context));
        items.push_str("]");
        if let Some(optional) = node.optional {
            items.push_str(match optional {
                TruePlusMinus::True => "?",
                TruePlusMinus::Plus => "+?",
                TruePlusMinus::Minus => "-?",
            });
        }
        items.extend(parse_type_annotation_with_colon_if_exists_for_type(&node.type_ann, context));
        if context.config.mapped_type_semi_colon {
            items.push_str(";");
        }
        items
    })));
    items.push_condition(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_info, Some(end_info)));
    items.push_str("}");
    items.push_info(end_info);
    return items;
}

fn parse_optional_type<'a>(node: &'a TsOptionalType, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.type_ann).into(), context));
    items.push_str("?");
    return items;
}

fn parse_qualified_name<'a>(node: &'a TsQualifiedName, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.left).into(), context));
    items.push_str(".");
    items.extend(parse_node((&node.right).into(), context));
    return items;
}

fn parse_parenthesized_type<'a>(node: &'a TsParenthesizedType, context: &mut Context<'a>) -> PrintItems {
    conditions::with_indent_if_start_of_line_indented(parser_helpers::new_line_group(
        parse_node_in_parens(
            (&node.type_ann).into(),
            |context| parse_node((&node.type_ann).into(), context),
            context
        )
    )).into()
}

fn parse_rest_type<'a>(node: &'a TsRestType, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("...");
    items.extend(parse_node((&node.type_ann).into(), context));
    return items;
}

fn parse_tuple_type<'a>(node: &'a TsTupleType, context: &mut Context<'a>) -> PrintItems {
    parse_array_like_nodes(ParseArrayLikeNodesOptions {
        parent_span: node.span,
        elements: node.elem_types.iter().map(|x| Some(x.into())).collect(),
        trailing_commas: context.config.tuple_type_trailing_commas,
    }, context)
}

fn parse_type_ann<'a>(node: &'a TsTypeAnn, context: &mut Context<'a>) -> PrintItems {
    parse_node((&node.type_ann).into(), context)
}

fn parse_type_param<'a>(node: &'a TsTypeParam, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    items.extend(parse_node((&node.name).into(), context));

    if let Some(constraint) = &node.constraint {
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str(if context.parent().kind() == NodeKind::TsMappedType {
                "in "
            } else {
                "extends "
            });
            items.extend(parse_node(constraint.into(), context));
            items
        }));
    }

    if let Some(default) = &node.default {
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line({
            let mut items = PrintItems::new();
            items.push_str("= ");
            items.extend(parse_node(default.into(), context));
            items
        }));
    }

    return items;
}

fn parse_type_param_instantiation<'a>(node: TypeParamNode<'a>, context: &mut Context<'a>) -> PrintItems {
    let parent_span = node.span();
    let params = node.params();
    let use_new_lines = get_use_new_lines(&parent_span, &params, context);
    let parsed_params = parse_parameter_list(params, use_new_lines, context);
    let mut items = PrintItems::new();

    items.push_str("<");
    items.extend(if use_new_lines {
        parser_helpers::surround_with_new_lines(parsed_params)
    } else {
        parsed_params
    });
    items.push_str(">");

    return items;

    fn parse_parameter_list<'a>(params: Vec<Node<'a>>, use_new_lines: bool, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        let params_count = params.len();

        for (i, param) in params.into_iter().enumerate() {
            if i > 0 {
                items.push_signal(if use_new_lines { Signal::NewLine } else { Signal::SpaceOrNewLine });
            }

            items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node_with_inner_parse(param, context, move |mut items| {
                if i < params_count - 1 {
                    items.push_str(",");
                }
                items
            }))));
        }

        items
    }

    fn get_use_new_lines(parent_span: &Span, params: &Vec<Node>, context: &mut Context) -> bool {
        if params.is_empty() {
            false
        } else {
            let first_param = &params[0];
            let angle_bracket_pos = parent_span.lo();
            node_helpers::get_use_new_lines_for_nodes(&angle_bracket_pos, first_param, context)
        }
    }
}

fn parse_type_operator<'a>(node: &'a TsTypeOperator, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str(match node.op {
        TsTypeOperatorOp::KeyOf => "keyof ",
        TsTypeOperatorOp::Unique => "unique ",
        TsTypeOperatorOp::ReadOnly => "readonly ",
    });
    items.extend(parse_node((&node.type_ann).into(), context));
    return items;
}

fn parse_type_predicate<'a>(node: &'a TsTypePredicate, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if node.asserts { items.push_str("asserts "); }
    items.extend(parse_node((&node.param_name).into(), context));
    items.push_str(" is ");
    items.extend(parse_node((&node.type_ann).into(), context));
    return items;
}

fn parse_type_query<'a>(node: &'a TsTypeQuery, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("typeof ");
    items.extend(parse_node((&node.expr_name).into(), context));
    return items;
}

fn parse_type_reference<'a>(node: &'a TsTypeRef, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.type_name).into(), context));
    if let Some(type_params) = &node.type_params {
        items.extend(parse_node(type_params.into(), context));
    }
    return items;
}

fn parse_union_type<'a>(node: &'a TsUnionType, context: &mut Context<'a>) -> PrintItems {
    parse_union_or_intersection_type(UnionOrIntersectionType {
        span: node.span,
        types: &node.types,
        is_union: true,
    }, context)
}

struct UnionOrIntersectionType<'a> {
    pub span: Span,
    pub types: &'a Vec<Box<TsType>>,
    pub is_union: bool,
}

fn parse_union_or_intersection_type<'a>(node: UnionOrIntersectionType<'a>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let use_new_lines = node_helpers::get_use_new_lines_for_nodes(&node.types[0], &node.types[1], context);
    let separator = if node.is_union { "|" } else { "&" };
    let is_ancestor_parenthesized_type = get_is_ancestor_parenthesized_type(context);
    let is_parent_union_or_intersection_type = match context.parent().kind() { NodeKind::TsUnionType | NodeKind::TsIntersectionType => true, _ => false };
    let mut last_type_node_span: Option<Span> = None;

    for (i, type_node) in node.types.iter().enumerate() {
        if i > 0 {
            items.push_signal(if use_new_lines { Signal::NewLine } else { Signal::SpaceOrNewLine });
        }

        let separator_token = if let Some(last_type_node_span) = last_type_node_span.replace(type_node.span()) {
            get_separator_token(&separator, &last_type_node_span, &node.span, context)
        } else {
            None
        };
        let parsed_node = {
            let mut items = PrintItems::new();
            let after_separator_info = Info::new("afterSeparatorInfo");
            if i > 0 {
                items.push_str(separator);
                items.push_info(after_separator_info);
            }
            if let Some(separator_token) = separator_token {
                items.extend(parse_trailing_comments(&separator_token, context));
            }
            if i > 0 {
                items.push_condition(Condition::new("afterSeparatorSpace", ConditionProperties {
                    condition: Box::new(move |condition_context| condition_resolvers::is_on_same_line(condition_context, &after_separator_info)),
                    true_path: Some(" ".into()),
                    false_path: None
                }));
            }
            items.extend(parse_node(type_node.into(), context));
            items
        };
        // probably something better needs to be done here, but htis is good enough for now
        if is_ancestor_parenthesized_type || i == 0 && !is_parent_union_or_intersection_type {
            items.extend(parsed_node);
        } else {
            items.push_condition(conditions::indent_if_start_of_line(parsed_node));
        }
    }

    return items;

    fn get_separator_token<'a>(separator: &str, last_type_node: &dyn Ranged, parent: &dyn Ranged, context: &mut Context<'a>) -> Option<&'a TokenAndSpan> {
        let token = context.token_finder.get_first_operator_after(last_type_node, separator);
        if let Some(token) = &token {
            if token.lo() > parent.hi() {
                return None;
            }
        }
        return token;
    }

    fn get_is_ancestor_parenthesized_type(context: &mut Context) -> bool {
        for ancestor in context.parent_stack.iter() {
            match ancestor {
                Node::TsUnionType(_) | Node::TsIntersectionType(_) => continue,
                Node::TsParenthesizedType(_) => return true,
                _ => return false,
            }
        }
        return false;
    }
}

/* comments */

fn parse_leading_comments<'a>(node: &dyn Spanned, context: &mut Context<'a>) -> PrintItems {
    let leading_comments = node.leading_comments(context);
    parse_comments_as_leading(node, leading_comments, context)
}

fn parse_comments_as_leading<'a>(node: &dyn Spanned, comments: CommentsIterator<'a>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if let Some(last_comment) = comments.get_last_comment() {
        let last_comment_previously_handled = context.has_handled_comment(&last_comment);

        items.extend(parse_comment_collection(comments, None, context));

        // todo: this doesn't seem exactly right...
        if !last_comment_previously_handled {
            let node_start_line = node.start_line(context);
            let last_comment_end_line = last_comment.end_line(context);
            if node_start_line > last_comment_end_line {
                items.push_signal(Signal::NewLine);

                if node_start_line - 1 > last_comment_end_line {
                    items.push_signal(Signal::NewLine);
                }
            }
            else if last_comment.kind == CommentKind::Block && node_start_line == last_comment_end_line {
                items.push_str(" ");
            }
        }
    }

    items
}

fn parse_trailing_comments_as_statements<'a>(node: &dyn Spanned, context: &mut Context<'a>) -> PrintItems {
    let unhandled_comments = get_trailing_comments_as_statements(node, context);
    parse_comment_collection(unhandled_comments.into_iter(), Some(node), context)
}

fn get_trailing_comments_as_statements<'a>(node: &dyn Spanned, context: &mut Context<'a>) -> Vec<&'a Comment> {
    let mut comments = Vec::new();
    let node_end_line = node.end_line(context);
    for comment in node.trailing_comments(context) {
        if !context.has_handled_comment(&comment) && node_end_line < comment.end_line(context) {
            comments.push(comment);
        }
    }
    comments
}

fn parse_comment_collection<'a, CIter>(comments: CIter, last_node: Option<&dyn Spanned>, context: &mut Context<'a>) -> PrintItems where CIter : Iterator<Item=&'a Comment> {
    let mut last_node = last_node;
    let mut items = PrintItems::new();
    for comment in comments {
        if !context.has_handled_comment(comment) {
            items.extend(parse_comment_based_on_last_node(comment, &last_node, context));
            last_node = Some(comment);
        }
    }
    items
}

fn parse_comment_based_on_last_node(comment: &Comment, last_node: &Option<&dyn Spanned>, context: &mut Context) -> PrintItems {
    let mut items = PrintItems::new();

    if let Some(last_node) = last_node {
        if comment.start_line(context) > last_node.end_line(context) {
            items.push_signal(Signal::NewLine);

            if comment.start_line(context) > last_node.end_line(context) + 1 {
                items.push_signal(Signal::NewLine);
            }
        } else if comment.kind == CommentKind::Line || last_node.text(context).starts_with("/*") {
            items.push_str(" ");
        }
    }

    if let Some(parsed_comment) = parse_comment(&comment, context) {
        items.extend(parsed_comment);
    }

    return items;
}

fn parse_comment(comment: &Comment, context: &mut Context) -> Option<PrintItems> {
    // only parse if handled
    if context.has_handled_comment(comment) {
        return None;
    }

    // mark handled and parse
    context.mark_comment_handled(comment);
    return Some(match comment.kind {
        CommentKind::Block => parse_comment_block(comment),
        CommentKind::Line => parse_comment_line(comment),
    });

    fn parse_comment_block(comment: &Comment) -> PrintItems {
        let mut items = PrintItems::new();
        items.push_str("/*");
        items.extend(parse_raw_string(&comment.text));
        items.push_str("*/");
        items
    }

    fn parse_comment_line(comment: &Comment) -> PrintItems {
        let mut items = PrintItems::new();
        items.push_str(&get_comment_text(&comment.text));
        items.push_signal(Signal::ExpectNewLine);
        return items;

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

fn parse_first_line_trailing_comments<'a>(node: &dyn Spanned, first_member: Option<&dyn Spanned>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let node_start_line = node.start_line(context);

    for comment in get_comments(&node, &first_member, context) {
        if comment.start_line(context) == node_start_line {
            if let Some(parsed_comment) = parse_comment(comment, context) {
                if comment.kind == CommentKind::Line {
                    items.push_str(" ");
                }
                items.extend(parsed_comment);
            }
        }
    }

    return items;

    fn get_comments<'a>(node: &dyn Spanned, first_member: &Option<&dyn Spanned>, context: &mut Context<'a>) -> Vec<&'a Comment> {
        let mut comments = Vec::new();
        // todo: inner comments?
        if let Some(first_member) = first_member {
            comments.extend(first_member.leading_comments(context));
        }
        comments.extend(node.trailing_comments(context));
        return comments;
    }
}

fn parse_trailing_comments<'a>(node: &dyn Spanned, context: &mut Context<'a>) -> PrintItems {
    // todo: handle comments for object expr, arrayexpr, and tstupletype?
    let trailing_comments = node.trailing_comments(context);
    parse_comments_as_trailing(node, trailing_comments, context)
}

fn parse_comments_as_trailing<'a>(node: &dyn Spanned, trailing_comments: CommentsIterator<'a>, context: &mut Context<'a>) -> PrintItems {
    // use the roslyn definition of trailing comments
    let node_end_line = node.end_line(context);
    let trailing_comments_on_same_line = trailing_comments.into_iter().filter(|c| c.start_line(context) == node_end_line).collect::<Vec<&'a Comment>>();
    let first_unhandled_comment = trailing_comments_on_same_line.iter().filter(|c| !context.has_handled_comment(&c)).next();
    let mut items = PrintItems::new();

    if let Some(first_unhandled_comment) = first_unhandled_comment {
        if first_unhandled_comment.kind == CommentKind::Block {
            items.push_str(" ");
        }
    }

    items.extend(parse_comment_collection(trailing_comments_on_same_line.into_iter(), Some(node), context));

    items
}

fn get_jsx_empty_expr_comments<'a>(node: &JSXEmptyExpr, context: &mut Context<'a>) -> CommentsIterator<'a> {
    node.span.hi().leading_comments(context)
}

/* helpers */

struct ParseArrayLikeNodesOptions<'a> {
    parent_span: Span,
    elements: Vec<Option<Node<'a>>>,
    trailing_commas: TrailingCommas,
}

fn parse_array_like_nodes<'a>(opts: ParseArrayLikeNodesOptions<'a>, context: &mut Context<'a>) -> PrintItems {
    let parent_span = opts.parent_span;
    let elements = opts.elements;
    let use_new_lines = get_use_new_lines(&parent_span, &elements, context);
    let force_trailing_commas = get_force_trailing_commas(opts.trailing_commas, use_new_lines);
    let mut items = PrintItems::new();

    items.push_str("[");
    if !elements.is_empty() {
        items.extend(parse_elements(&parent_span, elements, use_new_lines, force_trailing_commas, context));
    }
    items.push_str("]");

    return items;

    fn parse_elements<'a>(parent_span: &Span, elements: Vec<Option<Node<'a>>>, use_new_lines: bool, force_trailing_commas: bool, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        let elements_len = elements.len();

        if use_new_lines { items.push_signal(Signal::NewLine); }

        for (i, element) in elements.into_iter().enumerate() {
            if i > 0 && !use_new_lines {
                items.push_signal(Signal::SpaceOrNewLine);
            }

            let has_comma = force_trailing_commas || i < elements_len - 1;
            items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_element(&parent_span, element, has_comma, context))));

            if use_new_lines { items.push_signal(Signal::NewLine); }
        }

        return items;
    }

    fn parse_element<'a>(parent_span: &Span, element: Option<Node<'a>>, has_comma: bool, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        let comma_token = get_comma_token(parent_span, &element, context);

        if let Some(element) = element {
            items.extend(parse_node_with_inner_parse(element, context, move |mut items| {
                if has_comma { items.push_str(","); }
                items
            }));
        } else if has_comma {
            items.push_str(",");
        }

        // get the trailing comments after the comma token
        if let Some(comma_token) = &comma_token {
            items.extend(parse_trailing_comments(comma_token, context));
        }
        return items;

        fn get_comma_token<'a>(parent_span: &Span, element: &Option<Node<'a>>, context: &mut Context<'a>) -> Option<&'a TokenAndSpan> {
            if let Some(element) = &element {
                let comma_token = context.token_finder.get_next_token_if_comma(&element);
                if let Some(comma_token) = comma_token {
                    if comma_token.lo() > parent_span.hi() {
                        return None;
                    }
                }
                return comma_token;
            } else {
                // Not worth handling this scenario at the moment.
                return None;
            }
        }
    }

    fn get_use_new_lines(node: &dyn Ranged, elements: &Vec<Option<Node>>, context: &mut Context) -> bool {
        if elements.is_empty() {
            false
        } else {
            let open_bracket_token = context.token_finder.get_first_open_bracket_token_within(node).expect("Expected to find an open bracket token.");
            if let Some(first_node) = &elements[0] {
                node_helpers::get_use_new_lines_for_nodes(&open_bracket_token, first_node, context)
            } else {
                // todo: tests for this (ex. [\n,] -> [\n    ,\n])
                let first_comma = context.token_finder.get_first_comma_within(&node);
                if let Some(first_comma) = first_comma {
                    node_helpers::get_use_new_lines_for_nodes(&open_bracket_token, &first_comma, context)
                } else {
                    false
                }
            }
        }
    }
}

struct ParseMemberedBodyOptions<'a, FShouldUseBlankLine> where FShouldUseBlankLine : Fn(&Node, &Node, &mut Context) -> bool {
    span: Span,
    members: Vec<Node<'a>>,
    start_header_info: Option<Info>,
    brace_position: BracePosition,
    should_use_blank_line: FShouldUseBlankLine,
    trailing_commas: Option<TrailingCommas>
}

fn parse_membered_body<'a, FShouldUseBlankLine>(
    opts: ParseMemberedBodyOptions<'a, FShouldUseBlankLine>,
    context: &mut Context<'a>
) -> PrintItems
    where FShouldUseBlankLine : Fn(&Node, &Node, &mut Context) -> bool
{
    let mut items = PrintItems::new();
    let open_brace_token = context.token_finder.get_first_open_brace_token_before(&if opts.members.is_empty() { opts.span.hi() } else { opts.members[0].lo() });
    let close_brace_token_pos = BytePos(opts.span.hi().0 - 1);
    let has_members = !opts.members.is_empty();

    items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
        brace_position: opts.brace_position,
        open_brace_token: open_brace_token,
        start_header_info: opts.start_header_info,
    }, context));

    items.push_str("{");
    let after_open_brace_info = Info::new("afterOpenBrace");
    items.push_info(after_open_brace_info);
    let open_brace_trailing_comments = open_brace_token.trailing_comments(context);
    let open_brace_trailing_comments_ends_with_comment_block = open_brace_trailing_comments.get_last_comment().map(|x| x.kind == CommentKind::Block).unwrap_or(false);
    items.extend(parse_comments_as_trailing(&open_brace_token, open_brace_trailing_comments, context));
    items.extend(parser_helpers::with_indent({
        let mut items = PrintItems::new();
        if !opts.members.is_empty() || close_brace_token_pos.leading_comments(context).any(|c| !context.has_handled_comment(&c)) {
            items.push_signal(Signal::NewLine);
        }

        items.extend(parse_statements_or_members(ParseStatementsOrMembersOptions {
            inner_span: Span::new(open_brace_token.hi(), close_brace_token_pos.lo(), Default::default()),
            items: opts.members.into_iter().map(|node| (node, None)).collect(),
            should_use_space: None,
            should_use_new_line: None,
            should_use_blank_line: opts.should_use_blank_line,
            trailing_commas: opts.trailing_commas,
        }, context));

        items
    }));

    if opts.span.start_line(context) == opts.span.end_line(context) && !has_members {
        items.push_condition(if_true_or(
            "newLineIfDifferentLine",
            move |context| condition_resolvers::is_on_different_line(context, &after_open_brace_info),
            Signal::NewLine.into(),
            {
                if open_brace_trailing_comments_ends_with_comment_block {
                    Signal::SpaceOrNewLine.into()
                } else {
                    PrintItems::new()
                }
            }
        ));
    } else {
        items.push_signal(Signal::NewLine);
    }

    items.push_str("}");

    items
}

fn parse_statements<'a>(inner_span: Span, stmts: Vec<Node<'a>>, context: &mut Context<'a>) -> PrintItems {
    parse_statements_or_members(ParseStatementsOrMembersOptions {
        inner_span,
        items: stmts.into_iter().map(|stmt| (stmt, None)).collect(),
        should_use_space: None,
        should_use_new_line: None,
        should_use_blank_line: |previous, next, context| node_helpers::has_separating_blank_line(previous, next, context),
        trailing_commas: None,
    }, context)
}

struct ParseStatementsOrMembersOptions<'a, FShouldUseBlankLine> where FShouldUseBlankLine : Fn(&Node, &Node, &mut Context) -> bool {
    inner_span: Span,
    items: Vec<(Node<'a>, Option<PrintItems>)>,
    should_use_space: Option<Box<dyn Fn(&Node, &Node, &mut Context) -> bool>>,
    should_use_new_line: Option<Box<dyn Fn(&Node, &Node, &mut Context) -> bool>>,
    should_use_blank_line: FShouldUseBlankLine,
    trailing_commas: Option<TrailingCommas>,
}

fn parse_statements_or_members<'a, FShouldUseBlankLine>(
    opts: ParseStatementsOrMembersOptions<'a, FShouldUseBlankLine>,
    context: &mut Context<'a>
) -> PrintItems where FShouldUseBlankLine : Fn(&Node, &Node, &mut Context) -> bool
{
    let mut last_node: Option<Node> = None;
    let mut items = PrintItems::new();
    let children_len = opts.items.len();

    for (i, (node, optional_print_items)) in opts.items.into_iter().enumerate() {
        if let Some(last_node) = last_node {
            if should_use_new_line(&opts.should_use_new_line, &last_node, &node, context) {
                items.push_signal(Signal::NewLine);

                if (opts.should_use_blank_line)(&last_node, &node, context) {
                    items.push_signal(Signal::NewLine);
                }
            }
            else if let Some(should_use_space) = &opts.should_use_space {
                if should_use_space(&last_node, &node, context) {
                    items.push_signal(Signal::SpaceOrNewLine);
                }
            }
        }

        let end_info = Info::new("endStatementOrMemberInfo");
        context.end_statement_or_member_infos.push(end_info);
        items.extend(if let Some(print_items) = optional_print_items {
            print_items
        } else {
            let trailing_commas = opts.trailing_commas;
            parse_node_with_inner_parse(node.clone(), context, move |mut items| {
                if let Some(trailing_commas) = trailing_commas {
                    let force_trailing_commas = get_force_trailing_commas(trailing_commas, true);
                    if force_trailing_commas || i < children_len - 1 {
                        items.push_str(",");
                    }
                }
                items
            })
        });
        items.push_info(end_info);
        context.end_statement_or_member_infos.pop();

        last_node = Some(node);
    }

    if let Some(last_node) = &last_node {
        items.extend(parse_trailing_comments_as_statements(last_node, context));
    }

    if children_len == 0 {
        items.extend(parse_comment_collection(opts.inner_span.hi().leading_comments(context), None, context));
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

struct ParseParametersOrArgumentsOptions<'a> {
    nodes: Vec<Node<'a>>,
    prefer_hanging: bool,
    custom_close_paren: Option<PrintItems>,
}

fn parse_parameters_or_arguments<'a>(opts: ParseParametersOrArgumentsOptions<'a>, context: &mut Context<'a>) -> PrintItems {
    let nodes = opts.nodes;
    let start_info = Info::new("startParamsOrArgs");
    let end_info = Info::new("endParamsOrArgs");
    let use_new_lines = get_use_new_lines(&nodes, context);
    let prefer_hanging = opts.prefer_hanging;
    let param_start_infos: Rc<RefCell<Vec<Info>>> = Rc::new(RefCell::new(Vec::new()));
    // todo: something better in the core library in order to facilitate this
    let mut is_any_param_on_new_line_condition = {
        let param_start_infos = param_start_infos.clone();
        Condition::new_with_dependent_infos("isAnyParamOnNewLineCondition", ConditionProperties {
            condition: Box::new(move |condition_context| {
                if use_new_lines { return Some(true); }
                if prefer_hanging { return Some(false); }
                // check if any of the param/arg starts are at the beginning of the line
                for param_start_info in param_start_infos.borrow().iter() {
                    let param_start_info = condition_context.get_resolved_info(param_start_info)?;
                    if param_start_info.column_number == param_start_info.line_start_column_number {
                        return Some(true);
                    }
                }

                Some(false)
            }),
            false_path: None,
            true_path: None,
        }, vec![end_info])
    };
    let is_any_param_on_new_line_condition_ref = is_any_param_on_new_line_condition.get_reference();
    let is_multi_line_or_hanging = move |condition_context: &mut ConditionResolverContext| {
        return condition_context.get_resolved_condition(&is_any_param_on_new_line_condition_ref);
    };

    let mut items = PrintItems::new();
    items.push_str("(");
    items.push_info(start_info);
    items.push_condition(is_any_param_on_new_line_condition);

    let parse_comma_separated_values_result = parse_comma_separated_values(nodes, is_multi_line_or_hanging, context);
    param_start_infos.borrow_mut().extend(parse_comma_separated_values_result.item_start_infos);
    let param_list = parse_comma_separated_values_result.items.into_rc_path();
    items.push_condition(Condition::new("multiLineOrHanging", ConditionProperties {
        condition: Box::new(is_multi_line_or_hanging),
        true_path: Some(surround_with_new_lines(with_indent(param_list.clone().into()))),
        false_path: Some(param_list.into()),
    }));

    items.push_info(end_info);

    if let Some(custom_close_paren) = opts.custom_close_paren {
        items.extend(custom_close_paren);
    }
    else {
        items.push_str(")");
    }

    return items;

    fn get_use_new_lines(nodes: &Vec<Node>, context: &mut Context) -> bool {
        if nodes.is_empty() {
            return false;
        }

        let first_node = &nodes[0];
        // arrow function expressions might not have an open paren (ex. `a => a + 5`)
        let open_paren_token = context.token_finder.get_previous_token_if_open_paren(first_node);

        if let Some(open_paren_token) = open_paren_token {
            node_helpers::get_use_new_lines_for_nodes(&open_paren_token, first_node, context)
        } else {
            false
        }
    }
}

struct ParseCloseParenWithTypeOptions<'a> {
    start_info: Info,
    type_node: Option<Node<'a>>,
    type_node_separator: Option<PrintItems>,
}

fn parse_close_paren_with_type<'a>(opts: ParseCloseParenWithTypeOptions<'a>, context: &mut Context<'a>) -> PrintItems {
    // todo: clean this up a bit
    let type_node_start_info = Info::new("typeNodeStart");
    let has_type_node = opts.type_node.is_some();
    let type_node_end_info = Info::new("typeNodeEnd");
    let start_info = opts.start_info;
    let parsed_type_node = parse_type_node(opts.type_node, opts.type_node_separator, type_node_start_info, type_node_end_info, context);
    let mut items = PrintItems::new();

    items.push_condition(Condition::new("newLineIfHeaderHangingAndTypeNodeMultipleLines", ConditionProperties {
        condition: Box::new(move |context| {
            if !has_type_node { return Some(false); }

            if let Some(is_hanging) = condition_resolvers::is_hanging(context, &start_info, &None) {
                if let Some(is_multiple_lines) = condition_resolvers::is_multiple_lines(context, &type_node_start_info, &type_node_end_info) {
                    return Some(is_hanging && is_multiple_lines);
                }
            }
            return None;
        }),
        true_path: Some(Signal::NewLine.into()),
        false_path: None,
    }));
    items.push_str(")");
    items.extend(parsed_type_node);
    return items;

    fn parse_type_node<'a>(
        type_node: Option<Node<'a>>,
        type_node_separator: Option<PrintItems>,
        type_node_start_info: Info,
        type_node_end_info: Info,
        context: &mut Context<'a>
    ) -> PrintItems {
        let mut items = PrintItems::new();
        if let Some(type_node) = type_node {
            items.push_info(type_node_start_info);
            if let Some(type_node_separator) = type_node_separator {
                items.extend(type_node_separator);
            } else {
                if context.config.type_annotation_space_before_colon { items.push_str(" "); }
                items.push_str(": ");
            }
            items.extend(parse_node(type_node.into(), context));
            items.push_info(type_node_end_info);
        }
        return items;
    }
}

struct ParseCommaAndSeparatedValuesResult {
    items: PrintItems,
    item_start_infos: Vec<Info>,
}

fn parse_comma_separated_values<'a>(
    values: Vec<Node<'a>>,
    multi_line_or_hanging_condition_resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    context: &mut Context<'a>
) -> ParseCommaAndSeparatedValuesResult {
    let mut items = PrintItems::new();
    let mut item_start_infos = Vec::new();
    let values_count = values.len();

    for (i, value) in values.into_iter().enumerate() {
        let has_comma = i < values_count - 1;
        let parsed_value = parse_value(value, has_comma, context);
        let start_info = Info::new("itemStartInfo");
        item_start_infos.push(start_info);

        if i == 0 {
            if values_count > 1 {
                items.push_condition(if_false(
                    "is_not_start_of_line",
                    |context| Some(condition_resolvers::is_start_of_new_line(context)),
                    Signal::PossibleNewLine.into()
                ));
            }

            items.push_info(start_info);
            items.extend(parsed_value);
        } else {
            let parsed_value = parsed_value.into_rc_path();
            items.push_condition(Condition::new("multiLineOrHangingCondition", ConditionProperties {
                condition: Box::new(multi_line_or_hanging_condition_resolver.clone()),
                true_path: {
                    let mut items = PrintItems::new();
                    items.push_signal(Signal::NewLine);
                    items.push_info(start_info);
                    items.extend(parsed_value.clone().into());
                    Some(items)
                },
                false_path: {
                    let mut items = PrintItems::new();
                    items.push_signal(Signal::SpaceOrNewLine);
                    items.push_info(start_info);
                    items.push_condition(conditions::indent_if_start_of_line(parsed_value.into()));
                    Some(items)
                },
            }));
        }
    }

    return ParseCommaAndSeparatedValuesResult {
        items,
        item_start_infos,
    };

    fn parse_value<'a>(value: Node<'a>, has_comma: bool, context: &mut Context<'a>) -> PrintItems {
        parser_helpers::new_line_group(parse_node_with_inner_parse(value, context, move |mut items| {
            if has_comma { items.push_str(","); }
            items
        }))
    }
}

/// For some reason, some nodes don't have a TsTypeAnn, but instead of a Box<TsType>
fn parse_type_annotation_with_colon_if_exists_for_type<'a>(type_ann: &'a Option<Box<TsType>>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if let Some(type_ann) = type_ann {
        if context.config.type_annotation_space_before_colon {
            items.push_str(" ");
        }
        items.extend(parse_node_with_preceeding_colon(Some(type_ann.into()), context));
    }
    items
}

fn parse_type_annotation_with_colon_if_exists<'a>(type_ann: &'a Option<TsTypeAnn>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if let Some(type_ann) = type_ann {
        if context.config.type_annotation_space_before_colon {
            items.push_str(" ");
        }
        items.extend(parse_node_with_preceeding_colon(Some(type_ann.into()), context));
    }
    items
}

fn parse_node_with_preceeding_colon<'a>(node: Option<Node<'a>>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if let Some(node) = node {
        items.push_str(":");
        items.push_signal(Signal::SpaceOrNewLine);
        items.push_condition(conditions::indent_if_start_of_line(parse_node(node, context)));
    }
    items
}

struct ParseBraceSeparatorOptions<'a> {
    brace_position: BracePosition,
    open_brace_token: Option<&'a TokenAndSpan>,
    start_header_info: Option<Info>,
}

fn parse_brace_separator<'a>(opts: ParseBraceSeparatorOptions<'a>, context: &mut Context) -> PrintItems {
    match opts.brace_position {
        BracePosition::NextLineIfHanging => {
            if let Some(start_header_info) = opts.start_header_info {
                conditions::new_line_if_hanging_space_otherwise(conditions::NewLineIfHangingSpaceOtherwiseOptions {
                    start_info: start_header_info,
                    end_info: None,
                    space_char: None,
                }).into()
            } else {
                " ".into()
            }
        },
        BracePosition::SameLine => {
            " ".into()
        },
        BracePosition::NextLine => {
            Signal::NewLine.into()
        },
        BracePosition::Maintain => {
            if let Some(open_brace_token) = opts.open_brace_token {
                if node_helpers::is_first_node_on_line(open_brace_token, context) {
                    Signal::NewLine.into()
                } else {
                    " ".into()
                }
            } else {
                " ".into()
            }
        },
    }
}

fn parse_node_in_parens<'a, F>(first_inner_node: Node<'a>, inner_parse_node: F, context: &mut Context<'a>) -> PrintItems where F : Fn(&mut Context<'a>) -> PrintItems {
    let open_paren_token = context.token_finder.get_previous_token_if_open_paren(&first_inner_node);
    let use_new_lines = {
        if let Some(open_paren_token) = &open_paren_token {
            node_helpers::get_use_new_lines_for_nodes(open_paren_token, &first_inner_node, context)
        } else {
            false
        }
    };

    // disable hanging indent on the next binary expression if necessary
    if use_new_lines && first_inner_node.kind() == NodeKind::BinExpr {
        context.mark_disable_indent_for_next_bin_expr();
    }

    // the inner parse needs to be done after potentially disabling hanging indent
    return wrap_in_parens(inner_parse_node(context), use_new_lines);
}

fn wrap_in_parens(parsed_node: PrintItems, use_new_lines: bool) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("(");
    if use_new_lines {
        items.push_signal(Signal::NewLine);
        items.extend(parser_helpers::with_indent(parsed_node));
        items.push_signal(Signal::NewLine);
    } else {
        items.extend(parsed_node);
    }
    items.push_str(")");
    items
}

fn parse_extends_or_implements<'a>(text: &'a str, type_items: Vec<Node<'a>>, start_header_info: Info, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    if type_items.is_empty() {
        return items;
    }

    items.push_condition(conditions::new_line_if_multiple_lines_space_or_new_line_otherwise(start_header_info, None));
    // the newline group will force it to put the extends or implements on a new line
    items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group({
        let mut items = PrintItems::new();
        items.push_str(&format!("{} ", text));
        for (i, type_item) in type_items.into_iter().enumerate() {
            if i > 0 {
                items.push_str(",");
                items.push_signal(Signal::SpaceOrNewLine);
            }

            items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node(type_item, context))));
        }
        items
    })));

    return items;
}

struct ParseObjectLikeNodeOptions<'a> {
    node_span: Span,
    members: Vec<Node<'a>>,
    trailing_commas: Option<TrailingCommas>,
}

fn parse_object_like_node<'a>(opts: ParseObjectLikeNodeOptions<'a>, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();

    if opts.members.is_empty() {
        items.push_str("{}"); // todo: comments?
        return items;
    }

    let open_brace_token = context.token_finder.get_first_open_brace_token_within(&opts.node_span).expect("Expected to find an open brace token.");
    let close_brace_token = BytePos(opts.node_span.hi().0 - 1);
    let multi_line = node_helpers::get_use_new_lines_for_nodes(
        &open_brace_token,
        &opts.members[0],
        context
    );
    let separator: PrintItems = if multi_line { Signal::NewLine.into() } else { " ".into() };
    let separator = separator.into_rc_path();

    items.push_str("{");
    items.extend(separator.clone().into());

    if multi_line {
        items.extend(parser_helpers::with_indent(parse_statements_or_members(ParseStatementsOrMembersOptions {
            inner_span: Span::new(open_brace_token.hi(), close_brace_token.lo(), Default::default()),
            items: opts.members.into_iter().map(|member| (member.into(), None)).collect(),
            should_use_space: None,
            should_use_new_line: None,
            should_use_blank_line: |previous, next, context| node_helpers::has_separating_blank_line(previous, next, context),
            trailing_commas: opts.trailing_commas,
        }, context)));
    } else {
        let members_len = opts.members.len();
        for (i, member) in opts.members.into_iter().enumerate() {
            if i > 0 { items.push_signal(Signal::SpaceOrNewLine); }

            let trailing_commas = opts.trailing_commas;
            items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node_with_inner_parse(member, context, move |mut items| {
                if let Some(trailing_commas) = trailing_commas {
                    if i < members_len - 1 || get_force_trailing_commas(trailing_commas, multi_line) {
                        items.push_str(",");
                    }
                }
                items
            }))));
        }
    }

    items.extend(separator.into());
    items.push_str("}");

    return items;
}

struct MemberLikeExpr<'a> {
    left_node: Node<'a>,
    right_node: Node<'a>,
    is_computed: bool,
}

fn parse_for_member_like_expr<'a>(node: MemberLikeExpr<'a>, context: &mut Context<'a>) -> PrintItems {
    let use_new_line = node_helpers::get_use_new_lines_for_nodes(&node.left_node, &node.right_node, context);
    let mut items = PrintItems::new();
    let is_optional = context.parent().kind() == NodeKind::OptChainExpr;

    items.extend(parse_node(node.left_node, context));
    items.push_signal(if use_new_line { Signal::NewLine } else { Signal::PossibleNewLine });
    items.push_condition(conditions::indent_if_start_of_line({
        let mut items = PrintItems::new();

        if is_optional {
            items.push_str("?");
            if node.is_computed { items.push_str("."); }
        }
        items.push_str(if node.is_computed { "[" } else { "." });
        items.extend(parse_node(node.right_node, context));
        if node.is_computed { items.push_str("]"); }

        items
    }));

    return items;
}

fn parse_decorators<'a>(decorators: &'a Vec<Decorator>, is_inline: bool, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    if decorators.is_empty() {
        return items;
    }

    let use_new_lines = !is_inline
        && decorators.len() >= 2
        && node_helpers::get_use_new_lines_for_nodes(&decorators[0], &decorators[1], context);

    for (i, decorator) in decorators.iter().enumerate() {
        if i > 0 {
            items.push_signal(if use_new_lines {
                Signal::NewLine
            } else {
                Signal::SpaceOrNewLine
            });
        }

        let parsed_node = parse_node(decorator.into(), context);
        if is_inline {
            items.push_condition(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parsed_node)));
        } else {
            items.extend(parser_helpers::new_line_group(parsed_node));
        }
    }

    items.push_signal(if is_inline {
        Signal::SpaceOrNewLine
    } else {
        Signal::NewLine
    });

    return items;
}

fn parse_control_flow_separator(
    next_control_flow_position: NextControlFlowPosition,
    previous_node_block: &Span,
    token_text: &str,
    context: &mut Context
) -> PrintItems {
    let mut items = PrintItems::new();
    match next_control_flow_position {
        NextControlFlowPosition::SameLine => items.push_str(" "),
        NextControlFlowPosition::NextLine => items.push_signal(Signal::NewLine),
        NextControlFlowPosition::Maintain => {
            let token = context.token_finder.get_first_keyword_after(&previous_node_block, token_text);

            if token.is_some() && node_helpers::is_first_node_on_line(&token.unwrap(), context) {
                items.push_signal(Signal::NewLine);
            } else {
                items.push_str(" ");
            }
        }
    }
    return items;
}

struct ParseHeaderWithConditionalBraceBodyOptions<'a> {
    parent: &'a Span,
    body_node: Node<'a>,
    parsed_header: PrintItems,
    use_braces: UseBraces,
    brace_position: BracePosition,
    single_body_position: Option<SingleBodyPosition>,
    requires_braces_condition_ref: Option<ConditionReference>,
}

struct ParseHeaderWithConditionalBraceBodyResult {
    parsed_node: PrintItems,
    open_brace_condition_ref: ConditionReference,
}

fn parse_header_with_conditional_brace_body<'a>(opts: ParseHeaderWithConditionalBraceBodyOptions<'a>, context: &mut Context<'a>) -> ParseHeaderWithConditionalBraceBodyResult {
    let start_header_info = Info::new("startHeader");
    let end_header_info = Info::new("endHeader");
    let mut items = PrintItems::new();

    items.push_info(start_header_info);
    items.extend(opts.parsed_header);
    items.push_info(end_header_info);
    let result = parse_conditional_brace_body(ParseConditionalBraceBodyOptions {
        parent: opts.parent,
        body_node: opts.body_node,
        use_braces: opts.use_braces,
        brace_position: opts.brace_position,
        single_body_position: opts.single_body_position,
        requires_braces_condition_ref: opts.requires_braces_condition_ref,
        header_start_token: None,
        start_header_info: Some(start_header_info),
        end_header_info: Some(end_header_info),
    }, context);
    items.extend(result.parsed_node);

    return ParseHeaderWithConditionalBraceBodyResult {
        open_brace_condition_ref: result.open_brace_condition_ref,
        parsed_node: items,
    };
}

struct ParseConditionalBraceBodyOptions<'a> {
    parent: &'a Span,
    body_node: Node<'a>,
    use_braces: UseBraces,
    brace_position: BracePosition,
    single_body_position: Option<SingleBodyPosition>,
    requires_braces_condition_ref: Option<ConditionReference>,
    header_start_token: Option<&'a TokenAndSpan>,
    start_header_info: Option<Info>,
    end_header_info: Option<Info>,
}

struct ParseConditionalBraceBodyResult {
    parsed_node: PrintItems,
    open_brace_condition_ref: ConditionReference,
}

fn parse_conditional_brace_body<'a>(opts: ParseConditionalBraceBodyOptions<'a>, context: &mut Context<'a>) -> ParseConditionalBraceBodyResult {
    let start_header_info = opts.start_header_info;
    let end_header_info = opts.end_header_info;
    let requires_braces_condition = opts.requires_braces_condition_ref;
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
    let mut newline_or_space_condition = Condition::new("newLineOrSpace", ConditionProperties {
        condition: Box::new(move |condition_context| {
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
        }),
        true_path: Some(Signal::NewLine.into()),
        false_path: Some(" ".into()),
    });
    let newline_or_space_condition_ref = newline_or_space_condition.get_reference();
    let mut open_brace_condition = Condition::new("openBrace", ConditionProperties {
        condition: {
            let has_open_brace_token = open_brace_token.is_some();
            Box::new(move |condition_context| {
                match use_braces {
                    UseBraces::WhenNotSingleLine => condition_context.get_resolved_condition(&newline_or_space_condition_ref),
                    UseBraces::Maintain => Some(has_open_brace_token),
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
            let mut items = PrintItems::new();
            items.extend(parse_brace_separator(ParseBraceSeparatorOptions {
                brace_position: opts.brace_position,
                open_brace_token: open_brace_token,
                start_header_info,
            }, context));
            items.push_str("{");
            Some(items)
        },
        false_path: None,
    });
    let open_brace_condition_ref = open_brace_condition.get_reference();

    // parse body
    let mut items = PrintItems::new();
    items.push_condition(open_brace_condition);
    let parsed_comments = parse_comment_collection(header_trailing_comments.into_iter(), None, context);
    if !parsed_comments.is_empty() {
        items.push_str(" ");
        items.extend(parsed_comments);
    }
    items.push_condition(newline_or_space_condition);
    items.push_info(start_statements_info);

    if let Node::BlockStmt(body_node) = opts.body_node {
        items.extend(parser_helpers::with_indent({
            let mut items = PrintItems::new();
            // parse the remaining trailing comments inside because some of them are parsed already
            // by parsing the header trailing comments
            items.extend(parse_leading_comments(&body_node, context));
            items.extend(parse_statements(body_node.get_inner_span(context), body_node.stmts.iter().map(|x| x.into()).collect(), context));
            items
        }));
    } else {
        items.extend(parser_helpers::with_indent({
            let mut items = PrintItems::new();
            let body_node_span = opts.body_node.span();
            items.extend(parse_node(opts.body_node, context));
            items.extend(parse_trailing_comments(&body_node_span, context));
            items
        }));
    }

    items.push_info(end_statements_info);
    items.push_condition(Condition::new("closeBrace", ConditionProperties {
        condition: Box::new(move |condition_context| condition_context.get_resolved_condition(&open_brace_condition_ref)),
        true_path: Some({
            let mut items = PrintItems::new();
            items.push_condition(Condition::new("closeBraceNewLine", ConditionProperties {
                condition: Box::new(move |condition_context| {
                    let is_new_line = condition_context.get_resolved_condition(&newline_or_space_condition_ref)?;
                    if !is_new_line { return Some(false); }
                    let are_infos_equal = condition_resolvers::are_infos_equal(condition_context, &start_statements_info, &end_statements_info)?;
                    return Some(!are_infos_equal);
                }),
                true_path: Some(Signal::NewLine.into()),
                false_path: Some(Condition::new("closeBraceSpace", ConditionProperties {
                    condition: Box::new(move |condition_context| {
                        let is_new_line = condition_context.get_resolved_condition(&newline_or_space_condition_ref)?;
                        return Some(!is_new_line);
                    }),
                    true_path: Some(" ".into()),
                    false_path: None,
                }).into())
            }));
            items.push_str("}");
            items
        }),
        false_path: None,
    }));

    // return result
    return ParseConditionalBraceBodyResult {
        parsed_node: items,
        open_brace_condition_ref,
    };

    fn get_should_use_new_line<'a>(
        body_node: &Node,
        body_should_be_multi_line: bool,
        single_body_position: &Option<SingleBodyPosition>,
        header_start_token: &Option<&'a TokenAndSpan>,
        parent: &Span,
        context: &mut Context<'a>
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

        fn get_header_start_line<'a>(header_start_token: &Option<&'a TokenAndSpan>, parent: &Span, context: &mut Context<'a>) -> usize {
            if let Some(header_start_token) = header_start_token {
                return header_start_token.start_line(context);
            }
            return parent.start_line(context);
        }
    }

    fn get_body_should_be_multi_line<'a>(body_node: &Node<'a>, header_trailing_comments: &Vec<&'a Comment>, context: &mut Context<'a>) -> bool {
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

    fn get_header_trailing_comments<'a>(body_node: &Node<'a>, context: &mut Context<'a>) -> Vec<&'a Comment> {
        let mut comments = Vec::new();
        if let Node::BlockStmt(block_stmt) = body_node {
            let comment_line = body_node.leading_comments(context).filter(|c| c.kind == CommentKind::Line).next();
            if let Some(comment) = comment_line {
                comments.push(comment);
                return comments;
            }

            let open_brace_token = context.token_finder.get_first_open_brace_token_within(&block_stmt);
            let body_node_start_line = body_node.start_line(context);
            comments.extend(open_brace_token.trailing_comments(context).filter(|c| c.start_line(context) == body_node_start_line));
        } else {
            let leading_comments = body_node.leading_comments(context);
            let last_header_token_end = context.token_finder.get_previous_token_end_before(body_node);
            let last_header_token_end_line = last_header_token_end.end_line(context);
            comments.extend(leading_comments.filter(|c| c.start_line(context) <= last_header_token_end_line));
        }

        return comments;
    }

    fn get_open_brace_token<'a>(body_node: &Node<'a>, context: &mut Context<'a>) -> Option<&'a TokenAndSpan> {
        if let Node::BlockStmt(block_stmt) = body_node {
            context.token_finder.get_first_open_brace_token_within(&block_stmt)
        } else {
            None
        }
    }
}

struct ParseJsxWithOpeningAndClosingOptions<'a> {
    opening_element: Node<'a>,
    closing_element: Node<'a>,
    children: Vec<Node<'a>>,
}

fn parse_jsx_with_opening_and_closing<'a>(opts: ParseJsxWithOpeningAndClosingOptions<'a>, context: &mut Context<'a>) -> PrintItems {
    let use_multi_lines = get_use_multi_lines(&opts.opening_element, &opts.children, context);
    let children = opts.children.into_iter().filter(|c| match c {
        Node::JSXText(c) => !c.text(context).trim().is_empty(),
        _=> true,
    }).collect();
    let start_info = Info::new("startInfo");
    let end_info = Info::new("endInfo");
    let mut items = PrintItems::new();
    let inner_span = Span::new(opts.opening_element.span().hi(), opts.closing_element.span().lo(), Default::default());

    items.push_info(start_info);
    items.extend(parse_node(opts.opening_element, context));
    items.extend(parse_jsx_children(ParseJsxChildrenOptions {
        inner_span,
        children,
        parent_start_info: start_info,
        parent_end_info: end_info,
        use_multi_lines,
    }, context));
    items.extend(parse_node(opts.closing_element, context));
    items.push_info(end_info);

    return items;

    fn get_use_multi_lines(opening_element: &Node, children: &Vec<Node>, context: &mut Context) -> bool {
        if let Some(first_child) = children.get(0) {
            if let Node::JSXText(first_child) = first_child {
                if first_child.text(context).find("\n").is_some() {
                    return true;
                }
            }

            node_helpers::get_use_new_lines_for_nodes(opening_element, first_child, context)
        } else {
            false
        }
    }
}

struct ParseJsxChildrenOptions<'a> {
    inner_span: Span,
    children: Vec<Node<'a>>,
    parent_start_info: Info,
    parent_end_info: Info,
    use_multi_lines: bool,
}

fn parse_jsx_children<'a>(opts: ParseJsxChildrenOptions<'a>, context: &mut Context<'a>) -> PrintItems {
    // Need to parse the children here so they only get parsed once.
    // Nodes need to be only parsed once so that their comments don't end up in
    // the handled comments collection and the second time they won't be parsed out.
    let children = opts.children.into_iter().map(|c| (c.clone(), parse_node(c, context).into_rc_path())).collect();
    let parent_start_info = opts.parent_start_info;
    let parent_end_info = opts.parent_end_info;

    if opts.use_multi_lines {
        return parse_for_new_lines(children, opts.inner_span, context);
    }
    else {
        // decide whether newlines should be used or not
        return Condition::new("jsxChildrenNewLinesOrNot", ConditionProperties {
            condition: Box::new(move |condition_context| {
                // use newlines if the header is multiple lines
                let resolved_parent_start_info = condition_context.get_resolved_info(&parent_start_info)?;
                if resolved_parent_start_info.line_number < condition_context.writer_info.line_number {
                    return Some(true);
                }

                // use newlines if the entire jsx element is on multiple lines
                return condition_resolvers::is_multiple_lines(condition_context, &parent_start_info, &parent_end_info);
            }),
            true_path: Some(parse_for_new_lines(children.clone(), opts.inner_span, context)),
            false_path: Some(parse_for_single_line(children, context)),
        }).into();
    }

    fn parse_for_new_lines<'a>(children: Vec<(Node<'a>, Option<PrintItemPath>)>, inner_span: Span, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        let has_children = !children.is_empty();
        items.push_signal(Signal::NewLine);
        items.extend(parser_helpers::with_indent(parse_statements_or_members(ParseStatementsOrMembersOptions {
            inner_span,
            items: children.into_iter().map(|(a, b)| (a, Some(b.into()))).collect(),
            should_use_space: Some(Box::new(|previous, next, context| should_use_space(previous, next, context))),
            should_use_new_line: Some(Box::new(|previous, next, context| {
                if let Node::JSXText(next) = next {
                    return !utils::has_no_new_lines_in_leading_whitespace(next.text(context));
                }
                if let Node::JSXText(previous) = previous {
                    return !utils::has_no_new_lines_in_trailing_whitespace(previous.text(context));
                }
                return true;
            })),
            should_use_blank_line: |previous, next, context| {
                if let Node::JSXText(previous) = previous {
                    return utils::has_new_line_occurrences_in_trailing_whitespace(previous.text(context), 2);
                }
                if let Node::JSXText(next) = next {
                    return utils::has_new_line_occurrences_in_leading_whitespace(next.text(context), 2);
                }
                return node_helpers::has_separating_blank_line(previous, next, context);
            },
            trailing_commas: None,
        }, context)));

        if has_children {
            items.push_signal(Signal::NewLine);
        }

        return items;
    }

    fn parse_for_single_line<'a>(children: Vec<(Node<'a>, Option<PrintItemPath>)>, context: &mut Context<'a>) -> PrintItems {
        let mut items = PrintItems::new();
        if children.is_empty() {
            items.push_signal(Signal::PossibleNewLine);
        } else {
            let mut previous_child: Option<Node<'a>> = None;
            for (child, parsed_child) in children.into_iter() {
                if let Some(previous_child) = previous_child {
                    if should_use_space(&previous_child, &child, context) {
                        items.push_signal(Signal::SpaceOrNewLine);
                    }
                }

                items.extend(parsed_child.into());
                items.push_signal(Signal::PossibleNewLine);
                previous_child = Some(child);
            }
        }
        return items;
    }

    fn should_use_space(previous_element: &Node, next_element: &Node, context: &mut Context) -> bool {
        if let Node::JSXText(element) = previous_element {
            return element.text(context).ends_with(" ");
        }
        if let Node::JSXText(element) = next_element {
            return element.text(context).starts_with(" ");
        }
        return false;
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

fn get_force_trailing_commas(option: TrailingCommas, use_new_lines: bool) -> bool {
    match option {
        TrailingCommas::Always => true,
        TrailingCommas::OnlyMultiLine => use_new_lines,
        TrailingCommas::Never => false,
    }
}

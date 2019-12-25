extern crate dprint_core;

use std::rc::Rc;

use dprint_core::*;
use dprint_core::{parser_helpers::*,condition_resolvers};
use super::*;
use swc_ecma_ast::{CallExpr, Module, Expr, ExprStmt, BigInt, Bool, JSXText, Number, Regex, Str, ExprOrSuper, Ident, ExprOrSpread, TsTypeParamInstantiation};
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
    //println!("Node kind: {:?}", node.kind());
    parse_node_with_inner_parse(node, context, |items| items)
}

fn parse_node_with_inner_parse(node: Node, context: &mut Context, inner_parse: impl Fn(Vec<PrintItem>) -> Vec<PrintItem> + Clone + 'static) -> Vec<PrintItem> {
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
            /* expressions */
            Node::CallExpr(node) => parse_call_expression(node, context),
            Node::ExprOrSpread(node) => parse_expr_or_spread(node, context),
            Node::FnExpr(node) => vec![context.get_text_range(&node).text().into()], // todo
            Node::ArrowExpr(node) => vec![context.get_text_range(&node).text().into()], // todo
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
            /* statements */
            Node::ExprStmt(node) => parse_expr_stmt(node, context),
            /* types */
            Node::TsTypeAnn(node) => vec![context.get_text_range(&node).text().into()], // todo
            Node::TsTypeParamInstantiation(node) => parse_type_param_instantiation(node, context),
            /* unknown */
            Node::Unknown(text_range) => vec![context.get_text_range(&text_range).text().into()],
        }
    }
}

/* Common */

fn parse_identifier(node: Ident, context: &mut Context) -> Vec<PrintItem> {
    let mut items: Vec<PrintItem> = Vec::new();
    items.push((&node.sym as &str).into());

    if node.optional {
        items.push("?".into());
    }

    if let Some(type_ann) = node.type_ann {
        items.extend(parse_node(type_ann.into(), context));
    }

    items
}

/* Expressions */

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
        if !is_expr_func_expr(&node.args[1].expr) && !is_expr_arrow_func_expr(&node.args[1].expr) {
            return false;
        }

        return context.get_text_range(&node).start_line() == context.get_text_range(&node.args[1]).start_line();

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

/* Literals */

fn parse_big_int_literal(node: BigInt, context: &mut Context) -> Vec<PrintItem> {
    vec![context.get_text_range(&node).text().into()]
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
    vec![context.get_text_range(&node).text().into()]
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
    return parse_raw_string(&get_string_literal_text(&context.get_text_range(&node), context));

    fn get_string_literal_text(node: &TextRange, context: &mut Context) -> String {
        let string_value = get_string_value(&node, context);

        return match context.config.single_quotes {
            true => format!("'{}'", string_value.replace("'", "\\'")),
            false => format!("\"{}\"", string_value.replace("\"", "\\\"")),
        };

        fn get_string_value(node: &TextRange, context: &mut Context) -> String {
            let raw_string_text = node.text();
            let string_value = &raw_string_text[1..raw_string_text.len() - 1];
            let is_single_quote = raw_string_text.chars().next().unwrap() == '\'';

            match is_single_quote {
                true => string_value.replace("\\'", "'"),
                false => string_value.replace("\\\"", "\""),
            }
        }
    }
}

/* Module */

fn parse_module(node: Module, context: &mut Context) -> Vec<PrintItem> {
    parse_statements_or_members(ParseStatementOrMemberOptions {
        items: node.body.into_iter().map(|node| (context.get_text_range(&node), parse_node(node.into(), context))).collect(),
        last_node: Option::None,
        should_use_space: Option::None,
        should_use_new_line: Option::None,
        should_use_blank_line: Box::new(|previous, next, context| node_helpers::has_separating_blank_line(previous, next, context)),
    }, context)
}

/* Statements */

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

/* Types */

fn parse_type_param_instantiation(node: TsTypeParamInstantiation, context: &mut Context) -> Vec<PrintItem> {
    let use_new_lines = get_use_new_lines(&node, context);
    let mut items = Vec::new();
    let parsed_params = parse_parameter_list(node, use_new_lines, context);

    items.push("<".into());
    items.extend(if use_new_lines {
        parser_helpers::surround_with_new_lines(parsed_params)
    } else {
        parsed_params
    });
    items.push(">".into());

    return items;

    fn parse_parameter_list(node: TsTypeParamInstantiation, use_new_lines: bool, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();
        let params_count = node.params.len();

        for (i, box param) in node.params.into_iter().enumerate() {
            if i > 0 {
                items.push(if use_new_lines { PrintItem::NewLine } else { PrintItem::SpaceOrNewLine });
            }

            items.push(conditions::indent_if_start_of_line(parser_helpers::new_line_group(parse_node_with_inner_parse(param.into(), context, move |mut items| {
                if i < params_count - 1 {
                    items.push(",".into());
                }

                items
            }))).into());
        }

        items
    }

    fn get_use_new_lines(node: &TsTypeParamInstantiation, context: &mut Context) -> bool {
        if node.params.len() == 0 {
            false
        } else {
            let mut first_param = context.get_text_range(&node.params[0]);
            let angle_bracket_token = context.get_first_angle_bracket_token_before(&first_param);
            if let Some(angle_bracket_token) = angle_bracket_token {
                node_helpers::get_use_new_lines_for_nodes(&mut context.get_text_range(&angle_bracket_token.span), &mut first_param)
            } else {
                false
            }
        }
    }
}

/* Comments */

fn parse_leading_comments(node: &mut TextRange, context: &mut Context) -> Vec<PrintItem> {
    let leading_comments = node.leading_comments();
    parse_comments_as_leading(node, leading_comments, context)
}

fn parse_comments_as_leading(node: &mut TextRange, comments: Vec<Comment>, context: &mut Context) -> Vec<PrintItem> {
    if comments.is_empty() {
        return vec![];
    }

    let (mut last_comment, last_comment_kind) = {
        let last_comment_comment = comments.last().unwrap();
        let last_comment = context.get_text_range(&last_comment_comment.span);
        (last_comment, last_comment_comment.kind)
    };
    let last_comment_previously_handled = context.has_handled_comment(&last_comment);
    let mut items = Vec::new();

    items.extend(parse_comment_collection(&comments, &mut Option::None, context));

    if !last_comment_previously_handled {
        if node.start_line() > last_comment.end_line() {
            items.push(PrintItem::NewLine);

            if node.start_line() - 1 > last_comment.end_line() {
                items.push(PrintItem::NewLine);
            }
        }
        else if last_comment_kind == CommentKind::Block && node.start_line() == last_comment.end_line() {
            items.push(" ".into());
        }
    }

    items
}

fn parse_trailing_comments_as_statements(node: &mut TextRange, context: &mut Context) -> Vec<PrintItem> {
    let unhandled_comments = get_trailing_comments_as_statements(node, context);
    parse_comment_collection(&unhandled_comments, &mut Some(node.clone()), context)
}

fn get_trailing_comments_as_statements(node: &mut TextRange, context: &mut Context) -> Vec<Comment> {
    let mut items = Vec::new();
    for comment in node.trailing_comments() {
        let mut comment_range = context.get_text_range(&comment.span);
        if context.has_handled_comment(&comment_range) && node.end_line() < comment_range.end_line() {
            items.push(comment);
        }
    }
    items
}

fn parse_comment_collection(comments: &Vec<Comment>, last_node: &mut Option<TextRange>, context: &mut Context) -> Vec<PrintItem> {
    let mut last_node = last_node.clone();
    let mut items = Vec::new();
    for comment in comments {
        let comment_range = context.get_text_range(&comment.span);
        if !context.has_handled_comment(&comment_range) {
            items.extend(parse_comment_based_on_last_node(&comment, &mut last_node, context));
            last_node = Some(comment_range);
        }
    }
    items
}

fn parse_comment_based_on_last_node(comment: &Comment, last_node: &mut Option<TextRange>, context: &mut Context) -> Vec<PrintItem> {
    let mut items = Vec::new();

    if let Some(last_node) = last_node {
        let mut comment_range = context.get_text_range(&comment.span);
        if comment_range.start_line() > last_node.end_line() {
            items.push(PrintItem::NewLine);

            if comment_range.start_line() > last_node.end_line() + 1 {
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
    let comment_range = context.get_text_range(&comment.span);
    if context.has_handled_comment(&comment_range) {
        return Vec::new();
    }

    // mark handled and parse
    context.mark_comment_handled(&comment_range);
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

struct ParseStatementOrMemberOptions {
    items: Vec<(TextRange, Vec<PrintItem>)>,
    last_node: Option<TextRange>,
    should_use_space: Option<Box<dyn Fn(&mut TextRange, &mut TextRange, &mut Context) -> bool>>,
    should_use_new_line: Option<Box<dyn Fn(&mut TextRange, &mut TextRange, &mut Context) -> bool>>,
    should_use_blank_line: Box<dyn Fn(&mut TextRange, &mut TextRange, &mut Context) -> bool>,
}

fn parse_statements_or_members(opts: ParseStatementOrMemberOptions, context: &mut Context) -> Vec<PrintItem> {
    let mut last_node = opts.last_node;
    let mut items = Vec::new();

    for (mut item, parsed_node) in opts.items {
        if let Some(mut last_node) = last_node {
            if should_use_new_line(&opts.should_use_new_line, &mut last_node, &mut item, context) {
                items.push(PrintItem::NewLine);

                if (opts.should_use_blank_line)(&mut last_node, &mut item, context) {
                    items.push(PrintItem::NewLine);
                }
            }
            else if let Some(should_use_space) = &opts.should_use_space {
                if should_use_space(&mut last_node, &mut item, context) {
                    items.push(PrintItem::SpaceOrNewLine);
                }
            }
        }

        let end_info = Info::new("endStatementOrMemberInfo");
        items.extend(parsed_node);
        items.push(end_info.into());

        last_node = Some(item);
    }

    if let Some(mut last_node) = last_node {
        items.extend(parse_trailing_comments_as_statements(&mut last_node, context));
    }

    // todo: inner comments?

    return items;

    fn should_use_new_line(
        should_use_new_line: &Option<Box<dyn Fn(&mut TextRange, &mut TextRange, &mut Context) -> bool>>,
        last_node: &mut TextRange,
        next_node: &mut TextRange,
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

        let mut first_node = context.get_text_range(&nodes[0]);
        let open_paren_token = context.get_first_open_paren_token_before(&first_node);

        if let Some(open_paren_token) = open_paren_token {
            node_helpers::get_use_new_lines_for_nodes(&mut context.get_text_range(&open_paren_token.span), &mut first_node)
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

/* is functions */

fn is_expr_template(node: &Expr) -> bool {
    match node {
        Expr::Tpl(_) => true,
        _ => false
    }
}

fn is_expr_func_expr(node: &Expr) -> bool {
    match node {
        Expr::Fn(_) => true,
        _ => false
    }
}

fn is_expr_arrow_func_expr(node: &Expr) -> bool {
    match node {
        Expr::Arrow(_) => true,
        _ => false
    }
}

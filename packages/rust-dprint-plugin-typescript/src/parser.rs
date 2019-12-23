extern crate dprint_core;

use dprint_core::*;
use dprint_core::{parser_helpers::*};
use super::*;
use swc_ecma_ast::{Module, ModuleItem, Stmt, Expr, ExprStmt, Lit, Bool, JSXText, Number, Regex, Str};
use swc_common::{comments::{Comment, CommentKind}};

pub fn parse(source_file: ParsedSourceFile, config: TypeScriptConfiguration) -> Vec<PrintItem> {
    let mut context = Context::new(
        config,
        source_file.comments,
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

fn parse_module_item(item: ModuleItem, context: &mut Context) -> Vec<PrintItem> {
    match item {
        ModuleItem::Stmt(node) => parse_stmt(node, context),
        _ => parse_node(Node::Unknown(context.get_text_range(&item)), context), // todo: remove this
    }
}

fn parse_stmt(stmt: Stmt, context: &mut Context) -> Vec<PrintItem> {
    match stmt {
        Stmt::Expr(node) => parse_node(Node::ExprStmt(node), context),
        _ => parse_node(Node::Unknown(context.get_text_range(&stmt)), context), // todo: remove this
    }
}

fn parse_expr(expr: Expr, context: &mut Context) -> Vec<PrintItem> {
    match expr {
        Expr::Lit(lit) => parse_literal(lit, context),
        _ => parse_node(Node::Unknown(context.get_text_range(&expr)), context), // todo: remove this
    }
}

fn parse_literal(lit: Lit, context: &mut Context) -> Vec<PrintItem> {
    match lit {
        Lit::Bool(node) => parse_node(Node::Bool(node), context),
        Lit::JSXText(node) => parse_node(Node::JsxText(node), context),
        Lit::Null(node) => parse_node(Node::Null(node), context),
        Lit::Num(node) => parse_node(Node::Num(node), context),
        Lit::Regex(node) => parse_node(Node::Regex(node), context),
        Lit::Str(node) => parse_node(Node::Str(node), context),
    }
}

fn parse_node(node: Node, context: &mut Context) -> Vec<PrintItem> {
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
            /* literals */
            Node::Bool(node) => parse_bool_literal(&node),
            Node::JsxText(node) => parse_jsx_text(&node, context),
            Node::Null(_) => vec!["null".into()],
            Node::Num(node) => parse_num_literal(&node, context),
            Node::Regex(node) => parse_reg_exp_literal(&node, context),
            Node::Str(node) => parse_string_literal(&node, context),
            /* module */
            Node::Module(node) => parse_module(node, context),
            /* statements */
            Node::ExprStmt(node) => parse_expr_stmt(&node, context),
            /* unknown */
            Node::Unknown(text_range) => vec![text_range.text().into()],
        }
    }
}

/* Literals */

fn parse_bool_literal(node: &Bool) -> Vec<PrintItem> {
    vec![match node.value {
        true => "true",
        false => "false",
    }.into()]
}

fn parse_jsx_text(node: &JSXText, context: &mut Context) -> Vec<PrintItem> {
    vec![]
}

fn parse_num_literal(node: &Number, context: &mut Context) -> Vec<PrintItem> {
    vec![context.get_text_range(&node).text().into()]
}

fn parse_reg_exp_literal(node: &Regex, context: &mut Context) -> Vec<PrintItem> {
    // the exp and flags should not be nodes so just ignore that (swc issue #511)
    let mut items = Vec::new();
    items.push("/".into());
    items.push(String::from(&node.exp as &str).into());
    items.push("/".into());
    items.push(String::from(&node.flags as &str).into());
    items
}

fn parse_string_literal(node: &Str, context: &mut Context) -> Vec<PrintItem> {
    return parse_raw_string(&get_string_literal_text(&context.get_text_range(&node), context));

    fn get_string_literal_text(node: &TextRange, context: &mut Context) -> String {
        let string_value = get_string_value(&node, context);

        return match context.config.single_quotes {
            true => format!("'{}'", string_value.replace("'", "\\'")),
            false => format!("\"{}\"", string_value.replace("\"", "\\\"")),
        };

        fn get_string_value(node: &TextRange, context: &mut Context) -> String {
            let raw_string_text = node.text();
            let string_value = raw_string_text.chars().skip(1).take(raw_string_text.chars().count() - 2).collect::<String>();
            let is_double_quote = string_value.chars().next().unwrap() == '"';

            match is_double_quote {
                true => string_value.replace("\\\"", "\""),
                false => string_value.replace("\\'", "'"),
            }
        }
    }
}

/* Module */

fn parse_module(node: Module, context: &mut Context) -> Vec<PrintItem> {
    parse_statements_or_members(ParseStatementOrMemberOptions {
        items: node.body.into_iter().map(|node| (context.get_text_range(&node), parse_module_item(node, context))).collect(),
        last_node: Option::None,
        should_use_space: Option::None,
        should_use_new_line: Option::None,
        should_use_blank_line: Box::new(|previous, next, context| node_helpers::has_separating_blank_line(previous, next, context)),
    }, context)
}

/* Statements */

fn parse_expr_stmt(stmt: &ExprStmt, context: &mut Context) -> Vec<PrintItem> {
    if context.config.expression_statement_semi_colon {
        return parse_inner(&stmt, context);
    } else {
        return parse_for_prefix_semi_colon_insertion(&stmt, context);
    }

    fn parse_inner(stmt: &ExprStmt, context: &mut Context) -> Vec<PrintItem> {
        let mut items = Vec::new();
        items.extend(parse_expr(*stmt.expr.clone(), context));
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
        if node.line_start() > last_comment.line_end() {
            items.push(PrintItem::NewLine);

            if node.line_start() - 1 > last_comment.line_end() {
                items.push(PrintItem::NewLine);
            }
        }
        else if last_comment_kind == CommentKind::Block && node.line_start() == last_comment.line_end() {
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
        if context.has_handled_comment(&comment_range) && node.line_end() < comment_range.line_end() {
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
        if comment_range.line_start() > last_node.line_end() {
            items.push(PrintItem::NewLine);

            if comment_range.line_start() > last_node.line_end() + 1 {
                items.push(PrintItem::NewLine);
            }
        } else {
            items.push(" ".into());
        }
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

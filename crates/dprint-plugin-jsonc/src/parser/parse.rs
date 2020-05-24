use std::collections::HashSet;
use dprint_core::*;
use jsonc_parser::{parse_to_ast, ParseOptions};
use jsonc_parser::ast::*;
use jsonc_parser::common::{Position, Ranged, Range};
use jsonc_parser::tokens::{TokenAndRange};
use super::super::configuration::Configuration;
use super::context::Context;
use super::token_finder::TokenFinder;

pub fn parse_items(text: &str, config: &Configuration) -> Result<PrintItems, String> {
    let parse_result = parse_to_ast(text, &ParseOptions { comments: true, tokens: true });
    let parse_result = match parse_result {
        Ok(result) => result,
        Err(err) => return Err(dprint_core::utils::string_utils::format_diagnostic(
            Some((err.range.start, err.range.end)),
            &err.message,
            text
        )),
    };
    let comments = parse_result.comments.unwrap();
    let tokens = parse_result.tokens.unwrap();
    let node_value = parse_result.value;
    let mut context = Context {
        config,
        text,
        handled_comments: HashSet::new(),
        parent_stack: Vec::new(),
        current_node: None,
        comments: &comments,
        token_finder: TokenFinder::new(&tokens),
    };

    let mut items = PrintItems::new();
    if let Some(node_value) = &node_value {
        items.extend(parse_node(node_value.into(), &mut context));
        items.extend(parse_trailing_comments_as_statements(node_value, &mut context));
    } else {
        if let Some(comments) = comments.get(&0) {
            items.extend(parse_comments_as_statements(comments.iter(), None, &mut context));
        }
    }
    items.push_condition(conditions::if_true(
        "endOfFileNewLine",
        |context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0),
        Signal::NewLine.into()
    ));

    Ok(items)
}

fn parse_node<'a>(node: Node<'a>, context: &mut Context<'a>) -> PrintItems {
    parse_node_with_inner(node, context, |items, _| items)
}

fn parse_node_with_inner<'a>(
    node: Node<'a>,
    context: &mut Context<'a>,
    inner_parse: impl FnOnce(PrintItems, &mut Context<'a>) -> PrintItems
) -> PrintItems {
    // store info
    let past_current_node = context.current_node.replace(node.clone());
    let parent_end = past_current_node.as_ref().map(|n| n.end());
    let node_end = node.end();
    let is_root = past_current_node.is_none();

    if let Some(past_current_node) = past_current_node {
        context.parent_stack.push(past_current_node);
    }

    // parse item
    let mut items = PrintItems::new();

    // get the leading comments
    if let Some(comments) = context.comments.get(&node.start()) {
        items.extend(parse_comments_as_leading(&node, comments.iter(), context));
    }

    // parse the node
    items.extend(if has_ignore_comment(&node, context) {
        parser_helpers::parse_raw_string(node.text(context.text))
    } else {
        inner_parse(parse_node_inner(node.clone(), context), context)
    });

    // get the trailing comments
    if is_root || parent_end.is_some() && parent_end.unwrap() != node_end {
        if let Some(comments) = context.comments.get(&node_end) {
            items.extend(parse_comments_as_trailing(&node, comments.iter(), context));
        }
    }

    context.current_node = context.parent_stack.pop();

    return items;

    #[inline]
    fn parse_node_inner<'a>(node: Node<'a>, context: &mut Context<'a>) -> PrintItems {
        match node {
            Node::Array(node) => parse_array(node, context),
            Node::BooleanLit(node) => node.value.to_string().into(),
            Node::NullKeyword(_) => "null".into(),
            Node::NumberLit(node) => node.value.as_ref().into(),
            Node::Object(node) => parse_object(node, context),
            Node::ObjectProp(node) => parse_object_prop(node, context),
            Node::StringLit(node) => parse_string_lit(node, context),
        }
    }
}

fn parse_array<'a>(node: &'a Array, context: &mut Context<'a>) -> PrintItems {
    let force_multi_lines = node.range.start_line < node.elements.first().map(|p| p.start_line()).unwrap_or(node.range.start_line);

    parse_surrounded_by_tokens(|context| {
        let mut items = PrintItems::new();
        items.extend(parse_comma_separated_values(ParseCommaSeparatedValuesOptions {
            nodes: node.elements.iter().map(|x| Some(x.into())).collect(),
            prefer_hanging: false,
            force_use_new_lines: force_multi_lines,
            allow_blank_lines: true,
            single_line_space_at_start: false,
            single_line_space_at_end: false,
            custom_single_line_separator: None,
            multi_line_options: parser_helpers::MultiLineOptions::surround_newlines_indented(),
            force_possible_newline_at_start: false,
        }, context));
        items
    }, ParseSurroundedByTokensOptions {
        open_token: "[",
        close_token: "]",
        range: node.range.clone(),
        first_member: node.elements.first().map(|f| f.range()),
        prefer_single_line_when_empty: true,
    }, context)
}

fn parse_object<'a>(obj: &'a Object, context: &mut Context<'a>) -> PrintItems {
    let force_multi_lines = obj.range.start_line < obj.properties.first().map(|p| p.range.start_line).unwrap_or(obj.range.end_line);

    parse_surrounded_by_tokens(|context| {
        let mut items = PrintItems::new();
        items.extend(parse_comma_separated_values(ParseCommaSeparatedValuesOptions {
            nodes: obj.properties.iter().map(|x| Some(Node::ObjectProp(x))).collect(),
            prefer_hanging: false,
            force_use_new_lines: force_multi_lines,
            allow_blank_lines: true,
            single_line_space_at_start: true,
            single_line_space_at_end: true,
            custom_single_line_separator: None,
            multi_line_options: parser_helpers::MultiLineOptions::surround_newlines_indented(),
            force_possible_newline_at_start: false,
        }, context));
        items
    }, ParseSurroundedByTokensOptions {
        open_token: "{",
        close_token: "}",
        range: obj.range.clone(),
        first_member: obj.properties.first().map(|f| &f.range),
        prefer_single_line_when_empty: false,
    }, context)
}

fn parse_object_prop<'a>(node: &'a ObjectProp, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.extend(parse_node((&node.name).into(), context));
    items.push_str(": ");
    items.extend(parse_node((&node.value).into(), context));

    items
}

fn parse_string_lit<'a>(node: &'a StringLit, _: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_str("\"");
    items.push_str(&node.value.as_ref().replace("\"", "\\\""));
    items.push_str("\"");
    items
}

struct ParseCommaSeparatedValuesOptions<'a> {
    nodes: Vec<Option<Node<'a>>>,
    prefer_hanging: bool,
    force_use_new_lines: bool,
    allow_blank_lines: bool,
    single_line_space_at_start: bool,
    single_line_space_at_end: bool,
    custom_single_line_separator: Option<PrintItems>,
    multi_line_options: parser_helpers::MultiLineOptions,
    force_possible_newline_at_start: bool,
}

fn parse_comma_separated_values<'a>(
    opts: ParseCommaSeparatedValuesOptions<'a>,
    context: &mut Context<'a>
) -> PrintItems {
    let nodes = opts.nodes;
    let indent_width = context.config.indent_width;
    let compute_lines_span = opts.allow_blank_lines && opts.force_use_new_lines; // save time otherwise
    parser_helpers::parse_separated_values(|_| {
        let mut parsed_nodes = Vec::new();
        let nodes_count = nodes.len();
        for (i, value) in nodes.into_iter().enumerate() {
            let (allow_inline_multi_line, allow_inline_single_line) = if let Some(value) = &value {
                (value.kind() == NodeKind::Object, false)
            } else { (false, false) };
            let lines_span = if compute_lines_span {
                value.as_ref().map(|x| parser_helpers::LinesSpan{
                    start_line: context.start_line_with_comments(x),
                    end_line: context.end_line_with_comments(x),
                })
            } else { None };
            let items = parser_helpers::new_line_group({
                let parsed_comma = if i == nodes_count - 1 { PrintItems::new() } else { ",".into() };
                parse_comma_separated_value(value, parsed_comma, context)
            });
            parsed_nodes.push(parser_helpers::ParsedValue {
                items,
                lines_span,
                allow_inline_multi_line,
                allow_inline_single_line,
            });
        }

        parsed_nodes
    }, parser_helpers::ParseSeparatedValuesOptions {
        prefer_hanging: opts.prefer_hanging,
        force_use_new_lines: opts.force_use_new_lines,
        allow_blank_lines: opts.allow_blank_lines,
        single_line_space_at_start: opts.single_line_space_at_start,
        single_line_space_at_end: opts.single_line_space_at_end,
        single_line_separator: opts.custom_single_line_separator.unwrap_or(Signal::SpaceOrNewLine.into()),
        indent_width,
        multi_line_options: opts.multi_line_options,
        force_possible_newline_at_start: opts.force_possible_newline_at_start,
    }).items
}

fn parse_comma_separated_value<'a>(value: Option<Node<'a>>, parsed_comma: PrintItems, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let comma_token = get_comma_token(&value, context);

    if let Some(element) = value {
        let parsed_comma = parsed_comma.into_rc_path();
        items.extend(parse_node_with_inner(element, context, move |mut items, _| {
            // this Rc clone is necessary because we can't move the captured parsed_comma out of this closure
            items.push_optional_path(parsed_comma.clone());
            items
        }));
    } else {
        items.extend(parsed_comma);
    }

    // get the trailing comments after the comma token
    if let Some(comma_token) = comma_token {
        items.extend(parse_trailing_comments(comma_token, context));
    }

    return items;

    fn get_comma_token<'a>(element: &Option<Node<'a>>, context: &mut Context<'a>) -> Option<&'a TokenAndRange> {
        if let Some(element) = element {
            context.token_finder.get_next_token_if_comma(element)
        } else {
            None
        }
    }
}

struct ParseSurroundedByTokensOptions<'a> {
    open_token: &'static str,
    close_token: &'static str,
    range: Range,
    first_member: Option<&'a Range>,
    prefer_single_line_when_empty: bool,
}

fn parse_surrounded_by_tokens<'a>(
    parse_inner: impl FnOnce(&mut Context<'a>) -> PrintItems,
    opts: ParseSurroundedByTokensOptions<'a>,
    context: &mut Context<'a>
) -> PrintItems {
    let open_token_end = Position::new(opts.range.start + opts.open_token.len(), opts.range.start_line);
    let close_token_start = Position::new(opts.range.end - opts.close_token.len(), opts.range.end_line);

    // assert the tokens are in the place the caller says they are
    #[cfg(debug_assertions)]
    context.assert_text(opts.range.start, open_token_end.range.end, opts.open_token);
    #[cfg(debug_assertions)]
    context.assert_text(close_token_start.range.start, opts.range.end, opts.close_token);

    // parse
    let mut items = PrintItems::new();
    let open_token_start_line = opts.range.start_line;

    items.push_str(opts.open_token);
    if let Some(first_member) = opts.first_member {
        let first_member_start_line = first_member.start_line;
        if open_token_start_line < first_member_start_line {
            if let Some(trailing_comments) = context.comments.get(&open_token_end.start()) {
                items.extend(parse_first_line_trailing_comment(open_token_start_line, trailing_comments.iter(), context));
            }
        }
        items.extend(parse_inner(context));

        let before_trailing_comments_info = Info::new("beforeTrailingComments");
        items.push_info(before_trailing_comments_info);
        items.extend(parser_helpers::with_indent(parse_trailing_comments_as_statements(&open_token_end, context)));
        if let Some(leading_comments) = context.comments.get(&close_token_start.start()) {
            items.extend(parser_helpers::with_indent(parse_comments_as_statements(leading_comments.iter(), None, context)));
        }
        items.push_condition(conditions::if_true(
            "newLineIfHasCommentsAndNotStartOfNewLine",
            move |context| {
                let had_comments = !condition_resolvers::is_at_same_position(context, &before_trailing_comments_info)?;
                return Some(had_comments && !context.writer_info.is_start_of_line())
            },
            Signal::NewLine.into()
        ));
    } else {
        let is_single_line = open_token_start_line == opts.range.end_line;
        if let Some(comments) = context.comments.get(&open_token_end.start()) {
            // parse the trailing comment on the first line only if multi-line and if a comment line
            if !is_single_line {
                items.extend(parse_first_line_trailing_comment(open_token_start_line, comments.iter(), context));
            }

            // parse the comments
            if has_unhandled_comment(comments.iter(), context) {
                if is_single_line {
                    let indent_width = context.config.indent_width;
                    items.extend(parser_helpers::parse_separated_values(|_| {
                        let mut parsed_comments = Vec::new();
                        for c in comments.iter() {
                            let start_line = c.start_line();
                            let end_line = c.end_line();
                            if let Some(items) = parse_comment(c, context) {
                                parsed_comments.push(parser_helpers::ParsedValue {
                                    items,
                                    lines_span: Some(parser_helpers::LinesSpan { start_line, end_line }),
                                    allow_inline_multi_line: false,
                                    allow_inline_single_line: false,
                                });
                            }
                        }
                        parsed_comments
                    }, parser_helpers::ParseSeparatedValuesOptions {
                        prefer_hanging: false,
                        force_use_new_lines: !is_single_line,
                        allow_blank_lines: true,
                        single_line_space_at_start: false,
                        single_line_space_at_end: false,
                        single_line_separator: Signal::SpaceOrNewLine.into(),
                        indent_width,
                        multi_line_options: parser_helpers::MultiLineOptions::surround_newlines_indented(),
                        force_possible_newline_at_start: false,
                    }).items);
                } else {
                    items.push_signal(Signal::NewLine);
                    items.extend(parser_helpers::with_indent(parse_comments_as_statements(comments.iter(), None, context)));
                    items.push_signal(Signal::NewLine);
                }
            }
        } else {
            if !is_single_line && !opts.prefer_single_line_when_empty {
                items.push_signal(Signal::NewLine);
            }
        }
    }

    items.push_str(opts.close_token);

    return items;

    fn parse_first_line_trailing_comment<'a>(open_token_start_line: usize, comments: impl Iterator<Item=&'a Comment>, context: &mut Context) -> PrintItems {
        let mut items = PrintItems::new();
        let mut comments = comments;
        if let Some(first_comment) = comments.next() {
            if first_comment.kind() == CommentKind::Line && first_comment.start_line() == open_token_start_line {
                if let Some(parsed_comment) = parse_comment(&first_comment, context) {
                    items.push_signal(Signal::StartForceNoNewLines);
                    items.push_str(" ");
                    items.extend(parsed_comment);
                    items.push_signal(Signal::FinishForceNoNewLines);
                }
            }
        }
        items
    }
}

// Comments

fn has_unhandled_comment<'a>(comments: impl Iterator<Item=&'a Comment>, context: &mut Context) -> bool {
    comments.filter(|c| !context.has_handled_comment(c)).next().is_some()
}

fn parse_trailing_comments<'a>(node: &dyn Ranged, context: &mut Context<'a>) -> PrintItems {
    if let Some(trailing_comments) = context.comments.get(&node.end()) {
        parse_comments_as_trailing(node, trailing_comments.iter(), context)
    } else {
        PrintItems::new()
    }
}

fn parse_trailing_comments_as_statements<'a>(node: &dyn Ranged, context: &mut Context<'a>) -> PrintItems {
    let unhandled_comments = get_trailing_comments_as_statements(node, context);
    parse_comments_as_statements(unhandled_comments.into_iter(), Some(node), context)
}

fn get_trailing_comments_as_statements<'a>(node: &dyn Ranged, context: &mut Context<'a>) -> Vec<&'a Comment> {
    let mut comments = Vec::new();
    let node_end_line = node.end_line();
    if let Some(trailing_comments) = context.comments.get(&node.end()) {
        for comment in trailing_comments.iter() {
            if !context.has_handled_comment(comment) && node_end_line < comment.end_line() {
                comments.push(comment);
            }
        }
    }
    comments
}

fn parse_comments_as_statements<'a>(comments: impl Iterator<Item=&'a Comment>, last_node: Option<&dyn Ranged>, context: &mut Context<'a>) -> PrintItems {
    let mut last_node = last_node;
    let mut items = PrintItems::new();
    for comment in comments {
        if !context.has_handled_comment(comment) {
            items.extend(parse_comment_based_on_last_node(comment, &last_node, ParseCommentBasedOnLastNodeOptions {
                separate_with_newlines: true
            }, context));
            last_node = Some(comment);
        }
    }
    items
}

fn parse_comments_as_leading<'a>(node: &dyn Ranged, comments: impl Iterator<Item=&'a Comment>, context: &mut Context) -> PrintItems {
    let mut items = PrintItems::new();
    let comments = comments.filter(|c| !context.has_handled_comment(c)).collect::<Vec<_>>();

    if !comments.is_empty() {
        let last_comment = comments.last().unwrap();
        let last_comment_end_line = last_comment.end_line();
        let last_comment_kind = last_comment.kind();
        items.extend(parse_comment_collection(comments.into_iter(), None, Some(node), context));

        let node_start_line = node.start_line();
        if node_start_line > last_comment_end_line {
            items.push_signal(Signal::NewLine);

            if node_start_line - 1 > last_comment_end_line {
                items.push_signal(Signal::NewLine);
            }
        }
        else if last_comment_kind == CommentKind::Block && node_start_line == last_comment_end_line {
            items.push_signal(Signal::SpaceIfNotTrailing);
        }
    }

    items
}

fn parse_comments_as_trailing<'a>(node: &dyn Ranged, comments: impl Iterator<Item=&'a Comment>, context: &mut Context) -> PrintItems {
    // use the roslyn definition of trailing comments
    let node_end_line = node.end_line();
    let trailing_comments_on_same_line = comments
        .filter(|c| c.start_line() <= node_end_line)
        .collect::<Vec<_>>();

    let first_unhandled_comment = trailing_comments_on_same_line.iter().filter(|c| !context.has_handled_comment(c)).next();
    let mut items = PrintItems::new();

    if let Some(Comment::Block(_)) = first_unhandled_comment {
        items.push_str(" ");
    }

    items.extend(parse_comment_collection(trailing_comments_on_same_line.into_iter(), Some(node), None, context));

    items
}

fn parse_comment_collection<'a>(
    comments: impl Iterator<Item=&'a Comment>,
    last_node: Option<&dyn Ranged>,
    next_node: Option<&dyn Ranged>,
    context: &mut Context
) -> PrintItems {
    let mut last_node = last_node;
    let mut items = PrintItems::new();
    let next_node_start_line = next_node.map(|n| n.start_line());

    for comment in comments {
        if !context.has_handled_comment(comment) {
            items.extend(parse_comment_based_on_last_node(comment, &last_node, ParseCommentBasedOnLastNodeOptions {
                separate_with_newlines: if let Some(next_node_start_line) = next_node_start_line {
                    comment.start_line() != next_node_start_line
                } else {
                    false
                }
            }, context));
            last_node = Some(comment);
        }
    }

    items
}

struct ParseCommentBasedOnLastNodeOptions {
    separate_with_newlines: bool,
}

fn parse_comment_based_on_last_node(
    comment: &Comment,
    last_node: &Option<&dyn Ranged>,
    opts: ParseCommentBasedOnLastNodeOptions,
    context: &mut Context
) -> PrintItems {
    let mut items = PrintItems::new();
    let mut pushed_ignore_new_lines = false;

    if let Some(last_node) = last_node {
        let comment_start_line = comment.start_line();
        let last_node_end_line = last_node.end_line();

        if opts.separate_with_newlines || comment_start_line > last_node_end_line {
            items.push_signal(Signal::NewLine);

            if comment_start_line > last_node_end_line + 1 {
                items.push_signal(Signal::NewLine);
            }
        } else if comment.kind() == CommentKind::Line {
            items.push_signal(Signal::StartForceNoNewLines);
            items.push_str(" ");
            pushed_ignore_new_lines = true;
        } else if last_node.text(context.text).starts_with("/*") {
            items.push_str(" ");
        }
    }

    if let Some(parsed_comment) = parse_comment(&comment, context) {
        items.extend(parsed_comment);
    }

    if pushed_ignore_new_lines {
        items.push_signal(Signal::FinishForceNoNewLines);
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
    return Some(match comment {
        Comment::Block(comment) => parse_comment_block(comment),
        Comment::Line(comment) => parse_comment_line(comment, context),
    });

    fn parse_comment_block(comment: &CommentBlock) -> PrintItems {
        let mut items = PrintItems::new();
        items.push_str("/*");
        items.extend(parser_helpers::parse_raw_string(comment.text.as_ref()));
        items.push_str("*/");
        items
    }

    fn parse_comment_line(comment: &CommentLine, context: &mut Context) -> PrintItems {
        parser_helpers::parse_js_like_comment_line(&comment.text.as_ref(), context.config.comment_line_force_space_after_slashes)
    }
}

fn has_ignore_comment(node: &dyn Ranged, context: &Context) -> bool {
    if let Some(last_comment) = context.comments.get(&(node.start())).map(|c| c.last()).flatten() {
        parser_helpers::text_has_dprint_ignore(last_comment.text(), "dprint-ignore")
    } else {
        false
    }
}

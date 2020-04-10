use super::print_items::*;

pub fn surround_with_new_lines(item: PrintItems) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_signal(Signal::NewLine);
    items.extend(item);
    items.push_signal(Signal::NewLine);
    items
}

pub fn with_indent(item: PrintItems) -> PrintItems {
    with_indent_times(item, 1)
}

pub fn with_indent_times(item: PrintItems, times: u32) -> PrintItems {
    let mut items = PrintItems::new();
    for _ in 0..times { items.push_signal(Signal::StartIndent); }
    items.extend(item);
    for _ in 0..times { items.push_signal(Signal::FinishIndent); }
    items
}

pub fn with_no_new_lines(item: PrintItems) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_signal(Signal::StartForceNoNewLines);
    items.extend(item);
    items.push_signal(Signal::FinishForceNoNewLines);
    items
}

pub fn new_line_group(item: PrintItems) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_signal(Signal::StartNewLineGroup);
    items.extend(item);
    items.push_signal(Signal::FinishNewLineGroup);
    items
}

// todo: move these conditions to the conditions module

pub fn if_true(
    name: &'static str,
    resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    true_path: PrintItems
) -> Condition {
    Condition::new(name, ConditionProperties {
        true_path: Some(true_path),
        false_path: None,
        condition: Box::new(resolver.clone()),
    })
}

pub fn if_true_or(
    name: &'static str,
    resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    true_path: PrintItems,
    false_path: PrintItems
) -> Condition {
    Condition::new(name, ConditionProperties {
        true_path: Some(true_path),
        false_path: Some(false_path),
        condition: Box::new(resolver.clone())
    })
}

pub fn if_false(
    name: &'static str,
    resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    false_path: PrintItems
) -> Condition {
    Condition::new(name, ConditionProperties {
        true_path: None,
        false_path: Some(false_path),
        condition: Box::new(resolver.clone()),
    })
}

/// Parses a string as is and ignores its indent.
pub fn parse_raw_string(text: &str) -> PrintItems {
    let add_ignore_indent = text.find("\n").is_some();
    let mut items = PrintItems::new();
    if add_ignore_indent { items.push_signal(Signal::StartIgnoringIndent); }
    items.extend(parse_string(text));
    if add_ignore_indent { items.push_signal(Signal::FinishIgnoringIndent); }

    return items;
}

/// Parses a string to a series of PrintItems.
pub fn parse_string(text: &str) -> PrintItems {
    let mut items = PrintItems::new();
    let mut lines = text.lines().collect::<Vec<&str>>();

    // todo: this is kind of hacky...
    // using .lines() will remove the last line, so add it back if it exists
    if text.ends_with("\n") {
        lines.push("");
    }

    for i in 0..lines.len() {
        if i > 0 {
            items.push_signal(Signal::NewLine);
        }

        items.extend(parse_line(&lines[i]));
    }

    return items;

    fn parse_line(line: &str) -> PrintItems {
        let mut items = PrintItems::new();
        let parts = line.split("\t").collect::<Vec<&str>>();
        for i in 0..parts.len() {
            if i > 0 {
                items.push_signal(Signal::Tab);
            }
            if !parts[i].is_empty() {
                items.push_str(parts[i]);
            }
        }
        items.into()
    }
}

/// Surrounds the items with newlines and indentation if its on multiple lines.
/// Note: This currently inserts a possible newline at the start, but that might change or be made
/// conditional in the future.
pub fn surround_with_newlines_indented_if_multi_line(inner_items: PrintItems, indent_width: u8) -> PrintItems {
    let mut items = PrintItems::new();
    let start_info = Info::new("surroundWithNewLinesIndentedIfMultiLineStart");
    let end_info = Info::new("surroundWithNewLineIndentedsIfMultiLineEnd");
    let inner_items = inner_items.into_rc_path();

    items.push_info(start_info);
    items.push_condition(Condition::new_with_dependent_infos("newlineIfMultiLine", ConditionProperties {
        true_path: Some(surround_with_new_lines(with_indent(inner_items.clone().into()))),
        false_path: Some({
            let mut items = PrintItems::new();
            items.push_condition(super::conditions::if_above_width(
                indent_width,
                Signal::PossibleNewLine.into()
            ));
            items.extend(inner_items.into());
            items
        }),
        condition: Box::new(move |context| super::condition_resolvers::is_multiple_lines(context, &start_info, &end_info))
    }, vec![end_info]));
    items.push_info(end_info);

    items
}

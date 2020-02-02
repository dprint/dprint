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
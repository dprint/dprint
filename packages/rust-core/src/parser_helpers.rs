use super::print_items::*;

pub fn surround_with_new_lines(item: PrintItems) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_signal(Signal::NewLine);
    items.extend(item);
    items.push_signal(Signal::NewLine);
    items
}

pub fn with_indent(item: PrintItems) -> PrintItems {
    let mut items = PrintItems::new();
    items.push_signal(Signal::StartIndent);
    items.extend(item);
    items.push_signal(Signal::FinishIndent);
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

pub fn parse_raw_string(text: &str) -> PrintItems {
    let mut items = PrintItems::new();
    let mut has_ignored_indent = false;
    let mut lines = text.lines().collect::<Vec<&str>>();

    // todo: this is kind of hacky...
    // using .lines() will remove the last line, so add it back if it exists
    if text.ends_with("\n") {
        lines.push("");
    }

    for i in 0..lines.len() {
        if i > 0 {
            if !has_ignored_indent {
                items.push_signal(Signal::StartIgnoringIndent);
                has_ignored_indent = true;
            }

            items.push_signal(Signal::NewLine);
        }

        items.extend(parse_line(&lines[i]));
    }

    if has_ignored_indent {
        items.push_signal(Signal::FinishIgnoringIndent);
    }

    return items;

    fn parse_line(line: &str) -> PrintItems {
        let mut items = PrintItems::new();
        let parts = line.split("\t").collect::<Vec<&str>>();
        for i in 0..parts.len() {
            if i > 0 {
                items.push_signal(Signal::Tab);
            }
            items.push_str(parts[i]);
        }
        items.into()
    }
}

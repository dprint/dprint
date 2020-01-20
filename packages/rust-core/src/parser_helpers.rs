use super::print_items::*;

pub fn surround_with_new_lines(mut elements: Vec<PrintItem>) -> Vec<PrintItem> {
    elements.insert(0, PrintItem::NewLine);
    elements.push(PrintItem::NewLine);
    elements
}

pub fn with_indent(mut elements: Vec<PrintItem>) -> Vec<PrintItem> {
    elements.insert(0, PrintItem::StartIndent);
    elements.push(PrintItem::FinishIndent);
    elements
}

pub fn new_line_group(mut elements: Vec<PrintItem>) -> Vec<PrintItem> {
    elements.insert(0, PrintItem::StartNewLineGroup);
    elements.push(PrintItem::FinishNewLineGroup);
    elements
}

pub fn if_true(
    name: &'static str,
    resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    true_item: PrintItem
) -> PrintItem {
    Condition::new(name, ConditionProperties {
        true_path: Some(vec![true_item]),
        false_path: None,
        condition: Box::new(resolver.clone()),
    }).into()
}

pub fn if_true_or(
    name: &'static str,
    resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    true_item: PrintItem,
    false_item: PrintItem
) -> PrintItem {
    Condition::new(name, ConditionProperties {
        true_path: Some(vec![true_item]),
        false_path: Some(vec![false_item]),
        condition: Box::new(resolver.clone())
    }).into()
}

pub fn parse_raw_string(text: &str) -> Vec<PrintItem> {
    let mut items: Vec<PrintItem> = Vec::new();
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
                items.push(PrintItem::StartIgnoringIndent);
                has_ignored_indent = true;
            }

            items.push(PrintItem::NewLine);
        }

        items.extend(parse_line(&lines[i]));
    }

    if has_ignored_indent {
        items.push(PrintItem::FinishIgnoringIndent);
    }

    return items;

    fn parse_line(line: &str) -> Vec<PrintItem> {
        let mut items: Vec<PrintItem> = Vec::new();
        let parts = line.split("\t").collect::<Vec<&str>>();
        for i in 0..parts.len() {
            if i > 0 {
                items.push(PrintItem::Tab);
            }
            items.push(parts[i].into());
        }
        items
    }
}

pub fn prepend_if_has_items(items: Vec<PrintItem>, item: PrintItem) -> Vec<PrintItem> {
    let mut items = items;
    if !items.is_empty() {
        items.insert(0, item);
    }
    items
}

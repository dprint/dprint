use super::print_items::*;

pub fn surround_with_new_lines(item: PrintItem) -> PrintItem {
    let mut items = Vec::new();
    items.push(PrintItem::NewLine);
    items.push(item);
    items.push(PrintItem::NewLine);
    items.into()
}

pub fn with_indent(item: PrintItem) -> PrintItem {
    let mut items = Vec::new();
    items.push(PrintItem::StartIndent);
    items.push(item);
    items.push(PrintItem::FinishIndent);
    items.into()
}

pub fn new_line_group(item: PrintItem) -> PrintItem {
    let mut items = Vec::new();
    items.push(PrintItem::StartNewLineGroup);
    items.push(item);
    items.push(PrintItem::FinishNewLineGroup);
    items.into()
}

pub fn if_true(
    name: &'static str,
    resolver: impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static,
    true_item: PrintItem
) -> PrintItem {
    Condition::new(name, ConditionProperties {
        true_path: Some(true_item),
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
        true_path: Some(true_item),
        false_path: Some(false_item),
        condition: Box::new(resolver.clone())
    }).into()
}

pub fn parse_raw_string(text: &str) -> PrintItem {
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

        items.push(parse_line(&lines[i]));
    }

    if has_ignored_indent {
        items.push(PrintItem::FinishIgnoringIndent);
    }

    return items.into();

    fn parse_line(line: &str) -> PrintItem {
        let mut items: Vec<PrintItem> = Vec::new();
        let parts = line.split("\t").collect::<Vec<&str>>();
        for i in 0..parts.len() {
            if i > 0 {
                items.push(PrintItem::Tab);
            }
            items.push(parts[i].into());
        }
        items.into()
    }
}

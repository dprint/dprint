use std::rc::Rc;
use std::cell::RefCell;

use dprint_core::*;
use dprint_core::{parser_helpers::*,condition_resolvers};

use super::super::*;

// todo: improve then move down to the core library

pub struct ParseSeparatedValuesOptions {
    pub prefer_hanging: bool,
    pub force_use_new_lines: bool,
    pub single_line_space_at_start: bool,
    pub single_line_space_at_end: bool,
    pub single_line_separator: PrintItems,
    pub indent_width: u8,
    pub multi_line_style: MultiLineStyle,
}

#[derive(PartialEq, Clone, Copy)]
pub enum MultiLineStyle {
    SurroundNewlinesIndented,
    SameLineHangingIndented,
}

pub fn parse_separated_values(
    parse_nodes: impl FnOnce(&ConditionReference) -> Vec<PrintItems>,
    opts: ParseSeparatedValuesOptions
) -> PrintItems {
    let indent_width = opts.indent_width;
    let end_info = Info::new("endSeparatedValues");
    let node_start_infos: Rc<RefCell<Vec<Info>>> = Rc::new(RefCell::new(Vec::new()));

    let mut is_multi_line_or_hanging_condition = {
        if opts.force_use_new_lines { Condition::new_true() }
        else if opts.prefer_hanging {
            if opts.multi_line_style == MultiLineStyle::SameLineHangingIndented { Condition::new_false() }
            else { get_is_first_node_at_beginning_of_line_condition(node_start_infos.clone(), end_info) }
        } else { get_is_any_node_at_beginning_of_line_condition(node_start_infos.clone(), end_info) }
    };
    let is_multi_line_or_hanging_condition_ref = is_multi_line_or_hanging_condition.get_reference();
    let is_multi_line_or_hanging = is_multi_line_or_hanging_condition_ref.create_resolver();

    let mut items = PrintItems::new();
    items.push_condition(is_multi_line_or_hanging_condition);

    let parsed_nodes = (parse_nodes)(
        &is_multi_line_or_hanging_condition_ref // need to use a sized value it seems...
    );
    let has_nodes = !parsed_nodes.is_empty();
    let inner_parse_result = inner_parse(
        parsed_nodes,
        &is_multi_line_or_hanging,
        opts.single_line_separator,
        opts.multi_line_style
    );
    node_start_infos.borrow_mut().extend(inner_parse_result.item_start_infos);
    let node_list = inner_parse_result.items.into_rc_path();
    items.push_condition(Condition::new("multiLineOrHanging", ConditionProperties {
        condition: Box::new(is_multi_line_or_hanging),
        true_path: Some(match opts.multi_line_style {
            MultiLineStyle::SurroundNewlinesIndented => surround_with_new_lines(with_indent(node_list.clone().into())),
            MultiLineStyle::SameLineHangingIndented => node_list.clone().into(),
        }),
        false_path: Some({
            let mut items = PrintItems::new();
            if opts.single_line_space_at_start { items.push_str(" "); }
            if has_nodes {
                // place this after the space so the first item will start on a newline when there is a newline here
                items.push_condition(conditions::if_above_width(
                    indent_width + if opts.single_line_space_at_start { 1 } else { 0 },
                    Signal::PossibleNewLine.into()
                ));
            }
            items.extend(node_list.into());
            if opts.single_line_space_at_end { items.push_str(" "); }
            items
        }),
    }));

    items.push_info(end_info);

    return items;

    struct InnerParseResult {
        items: PrintItems,
        item_start_infos: Vec<Info>,
    }

    fn inner_parse(
        parsed_nodes: Vec<PrintItems>,
        is_multi_line_or_hanging: &(impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static),
        single_line_separator: PrintItems,
        multi_line_style: MultiLineStyle
    ) -> InnerParseResult {
        let mut items = PrintItems::new();
        let mut item_start_infos = Vec::new();
        let nodes_count = parsed_nodes.len();
        let single_line_separator = single_line_separator.into_rc_path();

        for (i, parsed_value) in parsed_nodes.into_iter().enumerate() {
            let start_info = Info::new("itemStartInfo");
            item_start_infos.push(start_info);

            if i == 0 {
                if multi_line_style == MultiLineStyle::SurroundNewlinesIndented && nodes_count > 1 {
                    items.push_condition(if_false(
                        "isNotStartOfLine",
                        |context| Some(condition_resolvers::is_start_of_line(context)),
                        Signal::PossibleNewLine.into()
                    ));
                }

                items.push_info(start_info);
                items.extend(parsed_value);
            } else {
                let parsed_value = parsed_value.into_rc_path();
                items.push_condition(Condition::new("multiLineOrHangingCondition", ConditionProperties {
                    condition: Box::new(is_multi_line_or_hanging.clone()),
                    true_path: {
                        let mut items = PrintItems::new();
                        items.push_signal(Signal::NewLine);
                        match multi_line_style {
                            MultiLineStyle::SurroundNewlinesIndented => {
                                items.push_info(start_info);
                                items.extend(parsed_value.clone().into());
                            },
                            MultiLineStyle::SameLineHangingIndented => {
                                items.push_condition(conditions::indent_if_start_of_line({
                                    let mut items = PrintItems::new();
                                    items.push_info(start_info);
                                    items.extend(parsed_value.clone().into());
                                    items
                                }));
                            },
                        }
                        Some(items)
                    },
                    false_path: {
                        let mut items = PrintItems::new();
                        items.extend(single_line_separator.clone().into()); // ex. Signal::SpaceOrNewLine
                        items.push_info(start_info);
                        items.push_condition(conditions::indent_if_start_of_line(parsed_value.into()));
                        Some(items)
                    },
                }));
            }
        }

        return InnerParseResult {
            items,
            item_start_infos,
        };
    }
}

fn get_is_first_node_at_beginning_of_line_condition(node_start_infos: Rc<RefCell<Vec<Info>>>, end_info: Info) -> Condition {
    Condition::new_with_dependent_infos("isFirstNodeAtBeginningOfLine", ConditionProperties {
        condition: Box::new(move |condition_context| {
            // check only if the first node is at the beginning of the line
            if let Some(first_node_start_info) = node_start_infos.borrow().iter().next() {
                let first_node_info = condition_context.get_resolved_info(first_node_start_info)?;
                if first_node_info.is_start_of_line() {
                    return Some(true);
                }
            }

            Some(false)
        }),
        false_path: None,
        true_path: None,
    }, vec![end_info])
}

fn get_is_any_node_at_beginning_of_line_condition(node_start_infos: Rc<RefCell<Vec<Info>>>, end_info: Info) -> Condition {
    Condition::new_with_dependent_infos("isAnyNodeAtBeginningOfLine", ConditionProperties {
        condition: Box::new(move |condition_context| {
            // check if any of the node starts are at the beginning of the line
            for node_start_info in node_start_infos.borrow().iter() {
                let node_start_info = condition_context.get_resolved_info(node_start_info)?;
                if node_start_info.is_start_of_line() {
                    return Some(true);
                }
            }

            Some(false)
        }),
        false_path: None,
        true_path: None,
    }, vec![end_info])
}

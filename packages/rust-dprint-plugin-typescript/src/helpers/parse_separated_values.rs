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
    /// Forces a possible newline at the start when there are nodes.
    /// If this isn't used, then a possible newline won't happen when
    /// the node is below the line
    pub force_possible_newline_at_start: bool,
}

pub struct ParseSeparatedValuesResult {
    pub items: PrintItems,
}

#[derive(PartialEq, Clone, Copy)]
pub enum MultiLineStyle {
    SurroundNewlinesIndented,
    SameLineStartWithHangingIndent,
    SameLineNoIndent,
    NewLineStart,
}

pub fn parse_separated_values(
    parse_nodes: impl FnOnce(&ConditionReference) -> Vec<PrintItems>,
    opts: ParseSeparatedValuesOptions
) -> ParseSeparatedValuesResult {
    let indent_width = opts.indent_width;
    let start_info = Info::new("startSeparatedValues");
    let end_info = Info::new("endSeparatedValues");
    let node_start_infos: Rc<RefCell<Vec<Info>>> = Rc::new(RefCell::new(Vec::new()));

    let mut is_start_standalone_line = get_is_start_standalone_line(node_start_infos.clone(), start_info, end_info);
    let is_start_standalone_line_ref = is_start_standalone_line.get_reference();
    let mut is_multi_line_or_hanging_condition = {
        if opts.force_use_new_lines { Condition::new_true() }
        else if opts.prefer_hanging {
            if opts.multi_line_style == MultiLineStyle::SameLineStartWithHangingIndent || opts.multi_line_style == MultiLineStyle::SameLineNoIndent { Condition::new_false() }
            else { get_is_multi_line_for_hanging(node_start_infos.clone(), is_start_standalone_line_ref, end_info) }
        } else { get_is_multi_line_for_multi_line(node_start_infos.clone(), is_start_standalone_line_ref, end_info) }
    };
    let is_multi_line_or_hanging_condition_ref = is_multi_line_or_hanging_condition.get_reference();
    let is_multi_line_or_hanging = is_multi_line_or_hanging_condition_ref.create_resolver();

    let mut items = PrintItems::new();
    items.push_info(start_info);
    items.push_condition(get_clearer_resolutions_on_start_change_condition(node_start_infos.clone(), start_info, end_info));
    items.push_condition(is_start_standalone_line);
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
            MultiLineStyle::SurroundNewlinesIndented => {
                let mut items = PrintItems::new();
                items.push_condition(if_true(
                    "newLineIndentedIfNotStandalone",
                    move |context| Some(!context.get_resolved_condition(&is_start_standalone_line_ref)?),
                    {
                        let mut items = PrintItems::new();
                        items.push_signal(Signal::NewLine);
                        items.push_signal(Signal::StartIndent);
                        items
                    }
                ));
                items.extend(node_list.clone().into());
                items.push_condition(if_true(
                    "newLineIndentedIfNotStandalone",
                    move |context| Some(!context.get_resolved_condition(&is_start_standalone_line_ref)?),
                    {
                        let mut items = PrintItems::new();
                        items.push_signal(Signal::FinishIndent);
                        items.push_signal(Signal::NewLine);
                        items
                    }
                ));
                items
            },
            MultiLineStyle::SameLineStartWithHangingIndent | MultiLineStyle::SameLineNoIndent => node_list.clone().into(),
            MultiLineStyle::NewLineStart => {
                let mut items = PrintItems::new();
                items.push_condition(if_false(
                    "isNotStartOfLine",
                    |context| Some(condition_resolvers::is_start_of_line(context)),
                    Signal::NewLine.into()
                ));
                items.extend(with_indent(node_list.clone().into()));
                items
            },
        }),
        false_path: Some({
            let mut items = PrintItems::new();
            let has_start_space = opts.single_line_space_at_start;
            if has_start_space {
                items.push_signal(Signal::SpaceIfNotTrailing);
                items.push_signal(Signal::PossibleNewLine);
            }
            if has_nodes {
                // place this after the space so the first item will start on a newline when there is a newline here
                items.push_condition(conditions::if_above_width(
                    if opts.force_possible_newline_at_start { 0 } else { indent_width + if has_start_space { 1 } else { 0 } },
                    Signal::PossibleNewLine.into()
                ));
            }
            items.extend(node_list.into());
            if opts.single_line_space_at_end { items.push_str(" "); }
            items
        }),
    }));

    items.push_info(end_info);

    return ParseSeparatedValuesResult {
        items,
    };

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
                            MultiLineStyle::SurroundNewlinesIndented | MultiLineStyle::NewLineStart | MultiLineStyle::SameLineNoIndent => {
                                items.push_info(start_info);
                                items.extend(parsed_value.clone().into());
                            },
                            MultiLineStyle::SameLineStartWithHangingIndent => {
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

fn get_clearer_resolutions_on_start_change_condition(node_start_infos: Rc<RefCell<Vec<Info>>>, start_info: Info, end_info: Info) -> Condition {
    let previous_position = RefCell::new((0, 0));
    Condition::new("clearWhenStartInfoChanges", ConditionProperties {
        condition: Box::new(move |condition_context| {
            // when the start info position changes, clear all the infos so they get re-evaluated again
            let start_info = condition_context.get_resolved_info(&start_info)?.clone();
            if start_info.get_line_and_column() != *previous_position.borrow() {
                for info in node_start_infos.borrow().iter() {
                    condition_context.clear_info(&info);
                }
                condition_context.clear_info(&end_info);
                previous_position.replace(start_info.get_line_and_column());
            }

            return None;
        }),
        false_path: None,
        true_path: None,
    })
}

fn get_is_start_standalone_line(node_start_infos: Rc<RefCell<Vec<Info>>>, standalone_start_info: Info, end_info: Info) -> Condition {
    Condition::new_with_dependent_infos("isStartStandaloneLine", ConditionProperties {
        condition: Box::new(move |condition_context| {
            if let Some(first_node_start_info) = node_start_infos.borrow().iter().next() {
                let standalone_start_info = condition_context.get_resolved_info(&standalone_start_info)?;
                let first_node_info = condition_context.get_resolved_info(first_node_start_info)?;
                return Some(first_node_info.is_start_of_line() && standalone_start_info.line_number == first_node_info.line_number);
            }

            Some(false)
        }),
        false_path: None,
        true_path: None,
    }, vec![end_info])
}

fn get_is_multi_line_for_hanging(node_start_infos: Rc<RefCell<Vec<Info>>>, is_start_standalone_line_ref: ConditionReference, end_info: Info) -> Condition {
    Condition::new_with_dependent_infos("isMultiLineForHanging", ConditionProperties {
        condition: Box::new(move |condition_context| {
            let is_start_standalone_line = condition_context.get_resolved_condition(&is_start_standalone_line_ref)?;
            if is_start_standalone_line {
                // check if the second node is on a newline
                if let Some(second_node_start_info) = node_start_infos.borrow().iter().skip(1).next() {
                    let second_node_start_info = condition_context.get_resolved_info(second_node_start_info)?;
                    return Some(second_node_start_info.is_start_of_line());
                }
            } else {
                // check if the first node is at the beginning of the line
                if let Some(first_node_start_info) = node_start_infos.borrow().iter().next() {
                    let first_node_start_info = condition_context.get_resolved_info(first_node_start_info)?;
                    return Some(first_node_start_info.is_start_of_line());
                }
            }

            Some(false)
        }),
        false_path: None,
        true_path: None,
    }, vec![end_info])
}

fn get_is_multi_line_for_multi_line(node_start_infos: Rc<RefCell<Vec<Info>>>, is_start_standalone_line_ref: ConditionReference, end_info: Info) -> Condition {
    Condition::new_with_dependent_infos("isMultiLineForMultiLine", ConditionProperties {
        condition: Box::new(move |condition_context| {
            let is_start_standalone_line = condition_context.get_resolved_condition(&is_start_standalone_line_ref)?;
            for (i, node_start_info) in node_start_infos.borrow().iter().enumerate() {
                let node_start_info = condition_context.get_resolved_info(node_start_info)?;
                // ignore, it will always be at the start of the line
                if i == 0 && is_start_standalone_line { continue; }

                // check if any of the node starts are at the beginning of the line
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

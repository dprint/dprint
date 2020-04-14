use std::rc::Rc;
use std::cell::RefCell;

use dprint_core::*;
use dprint_core::{parser_helpers::*,condition_resolvers};

use super::super::*;

// todo: improve then move down to the core library

pub struct ParseSeparatedValuesOptions {
    pub prefer_hanging: bool,
    pub force_use_new_lines: bool,
    pub allow_blank_lines: bool,
    pub single_line_space_at_start: bool,
    pub single_line_space_at_end: bool,
    pub single_line_separator: PrintItems,
    pub indent_width: u8,
    pub multi_line_style: MultiLineStyle,
    /// Forces a possible newline at the start when there are values.
    /// If this isn't used, then a possible newline won't happen when
    /// the value is below the line
    pub force_possible_newline_at_start: bool,
}

pub struct ParsedValue {
    pub items: PrintItems,
    pub lines_span: Option<LinesSpan>,
    /// Whether this value is allowed to start on the same line as the
    /// previous token and finish on the same line as the next token
    /// when multi-line.
    pub allow_inline_multi_line: bool,
    /// Whether this node is allowed to start on the same line as the
    /// previous token and finish on the same line as the next token
    /// when it is single line. In other words, it being on a single line
    /// won't trigger all the values to be multi-line.
    pub allow_inline_single_line: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct LinesSpan {
    pub start_line: usize,
    pub end_line: usize,
}

impl ParsedValue {
    /// Use this when you don't care about blank lines.
    pub fn from_items(items: PrintItems) -> ParsedValue {
        ParsedValue {
            items,
            lines_span: None,
            allow_inline_multi_line: false,
            allow_inline_single_line: false,
        }
    }
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

struct ParsedValueData {
    start_info: Info,
    allow_inline_multi_line: bool,
    allow_inline_single_line: bool,
}

impl MultiLineStyle {
    pub fn same_line_start(&self) -> bool {
        match self {
            MultiLineStyle::SameLineStartWithHangingIndent | MultiLineStyle::SameLineNoIndent => true,
            _ => false,
        }
    }
}

pub fn parse_separated_values(
    parse_values: impl FnOnce(&ConditionReference) -> Vec<ParsedValue>,
    opts: ParseSeparatedValuesOptions
) -> ParseSeparatedValuesResult {
    let indent_width = opts.indent_width;
    let start_info = Info::new("startSeparatedValues");
    let end_info = Info::new("endSeparatedValues");
    let value_datas: Rc<RefCell<Vec<ParsedValueData>>> = Rc::new(RefCell::new(Vec::new()));

    let mut is_start_standalone_line = get_is_start_standalone_line(value_datas.clone(), start_info, end_info);
    let is_start_standalone_line_ref = is_start_standalone_line.get_reference();
    let mut is_multi_line_or_hanging_condition = {
        if opts.force_use_new_lines { Condition::new_true() }
        else if opts.prefer_hanging {
            if opts.multi_line_style == MultiLineStyle::SameLineStartWithHangingIndent || opts.multi_line_style == MultiLineStyle::SameLineNoIndent { Condition::new_false() }
            else { get_is_multi_line_for_hanging(value_datas.clone(), is_start_standalone_line_ref, end_info) }
        } else {
            if opts.multi_line_style == MultiLineStyle::SameLineStartWithHangingIndent {
                get_spans_multiple_lines(value_datas.clone(), end_info)
            } else {
                get_is_multi_line_for_multi_line(start_info, value_datas.clone(), is_start_standalone_line_ref, end_info)
            }
        }
    };
    let is_multi_line_or_hanging_condition_ref = is_multi_line_or_hanging_condition.get_reference();
    let is_multi_line_or_hanging = is_multi_line_or_hanging_condition_ref.create_resolver();

    let mut items = PrintItems::new();
    items.push_info(start_info);
    items.push_condition(get_clearer_resolutions_on_start_change_condition(value_datas.clone(), start_info, end_info));
    items.push_condition(is_start_standalone_line);
    items.push_condition(is_multi_line_or_hanging_condition);

    let parsed_values = (parse_values)(
        &is_multi_line_or_hanging_condition_ref // need to use a sized value it seems...
    );
    let has_values = !parsed_values.is_empty();
    let inner_parse_result = inner_parse(
        parsed_values,
        &is_multi_line_or_hanging,
        opts.single_line_separator,
        opts.multi_line_style,
        opts.allow_blank_lines,
    );
    value_datas.borrow_mut().extend(inner_parse_result.value_datas);
    let parsed_values_items = inner_parse_result.items.into_rc_path();
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
                items.extend(parsed_values_items.clone().into());
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
            MultiLineStyle::SameLineStartWithHangingIndent | MultiLineStyle::SameLineNoIndent => parsed_values_items.clone().into(),
            MultiLineStyle::NewLineStart => {
                let mut items = PrintItems::new();
                items.push_condition(if_false(
                    "isNotStartOfLine",
                    |context| Some(condition_resolvers::is_start_of_line(context)),
                    Signal::NewLine.into()
                ));
                items.extend(with_indent(parsed_values_items.clone().into()));
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
            if has_values && !opts.multi_line_style.same_line_start() {
                // place this after the space so the first item will start on a newline when there is a newline here
                items.push_condition(conditions::if_above_width(
                    if opts.force_possible_newline_at_start { 0 } else { indent_width + if has_start_space { 1 } else { 0 } },
                    Signal::PossibleNewLine.into()
                ));
            }
            items.extend(parsed_values_items.into());
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
        value_datas: Vec<ParsedValueData>,
    }

    fn inner_parse(
        parsed_values: Vec<ParsedValue>,
        is_multi_line_or_hanging: &(impl Fn(&mut ConditionResolverContext) -> Option<bool> + Clone + 'static),
        single_line_separator: PrintItems,
        multi_line_style: MultiLineStyle,
        allow_blank_lines: bool,
    ) -> InnerParseResult {
        let mut items = PrintItems::new();
        let mut value_datas = Vec::new();
        let values_count = parsed_values.len();
        let single_line_separator = single_line_separator.into_rc_path();
        let mut last_lines_span: Option<LinesSpan> = None;

        for (i, parsed_value) in parsed_values.into_iter().enumerate() {
            let start_info = Info::new("valueStartInfo");
            value_datas.push(ParsedValueData {
                start_info,
                allow_inline_multi_line: parsed_value.allow_inline_multi_line,
                allow_inline_single_line: parsed_value.allow_inline_single_line,
            });

            if i == 0 {
                if multi_line_style == MultiLineStyle::SurroundNewlinesIndented && values_count > 1 {
                    items.push_condition(if_false(
                        "isNotStartOfLine",
                        |context| Some(condition_resolvers::is_start_of_line(context)),
                        Signal::PossibleNewLine.into()
                    ));
                }

                items.push_info(start_info);
                items.extend(parsed_value.items);
            } else {
                let use_blank_line = if let Some(last_lines_span) = last_lines_span {
                    if let Some(current_lines_span) = parsed_value.lines_span {
                        allow_blank_lines && last_lines_span.end_line < current_lines_span.start_line - 1
                    } else { false }
                } else { false };
                let parsed_value = parsed_value.items.into_rc_path();
                items.push_condition(Condition::new("multiLineOrHangingCondition", ConditionProperties {
                    condition: Box::new(is_multi_line_or_hanging.clone()),
                    true_path: {
                        let mut items = PrintItems::new();
                        if use_blank_line { items.push_signal(Signal::NewLine); }
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

            last_lines_span = parsed_value.lines_span;
        }

        return InnerParseResult {
            items,
            value_datas,
        };
    }
}

fn get_clearer_resolutions_on_start_change_condition(value_datas: Rc<RefCell<Vec<ParsedValueData>>>, start_info: Info, end_info: Info) -> Condition {
    let previous_position = RefCell::new((0, 0));
    Condition::new("clearWhenStartInfoChanges", ConditionProperties {
        condition: Box::new(move |condition_context| {
            // when the start info position changes, clear all the infos so they get re-evaluated again
            let start_info = condition_context.get_resolved_info(&start_info)?.clone();
            if start_info.get_line_and_column() != *previous_position.borrow() {
                for value_data in value_datas.borrow().iter() {
                    condition_context.clear_info(&value_data.start_info);
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

fn get_is_start_standalone_line(value_datas: Rc<RefCell<Vec<ParsedValueData>>>, standalone_start_info: Info, end_info: Info) -> Condition {
    Condition::new_with_dependent_infos("isStartStandaloneLine", ConditionProperties {
        condition: Box::new(move |condition_context| {
            if let Some(first_value_data) = value_datas.borrow().iter().next() {
                let standalone_start_info = condition_context.get_resolved_info(&standalone_start_info)?;
                let first_value_info = condition_context.get_resolved_info(&first_value_data.start_info)?;
                return Some(first_value_info.is_start_of_line() && standalone_start_info.line_number == first_value_info.line_number);
            }

            Some(false)
        }),
        false_path: None,
        true_path: None,
    }, vec![end_info])
}

fn get_is_multi_line_for_hanging(value_datas: Rc<RefCell<Vec<ParsedValueData>>>, is_start_standalone_line_ref: ConditionReference, end_info: Info) -> Condition {
    Condition::new_with_dependent_infos("isMultiLineForHanging", ConditionProperties {
        condition: Box::new(move |condition_context| {
            let is_start_standalone_line = condition_context.get_resolved_condition(&is_start_standalone_line_ref)?;
            if is_start_standalone_line {
                // check if the second value is on a newline
                if let Some(second_value_data) = value_datas.borrow().iter().skip(1).next() {
                    let second_value_start_info = condition_context.get_resolved_info(&second_value_data.start_info)?;
                    return Some(second_value_start_info.is_start_of_line());
                }
            } else {
                // check if the first value is at the beginning of the line
                if let Some(first_value_data) = value_datas.borrow().iter().next() {
                    let first_value_start_info = condition_context.get_resolved_info(&first_value_data.start_info)?;
                    return Some(first_value_start_info.is_start_of_line());
                }
            }

            Some(false)
        }),
        false_path: None,
        true_path: None,
    }, vec![end_info])
}

fn get_is_multi_line_for_multi_line(start_info: Info, value_datas: Rc<RefCell<Vec<ParsedValueData>>>, is_start_standalone_line_ref: ConditionReference, end_info: Info) -> Condition {
    return Condition::new_with_dependent_infos("isMultiLineForMultiLine", ConditionProperties {
        condition: Box::new(move |condition_context| {
            // todo: This is slightly confusing because it works on the "last" value rather than the current
            let is_start_standalone_line = condition_context.get_resolved_condition(&is_start_standalone_line_ref)?;
            let start_info = condition_context.get_resolved_info(&start_info)?;
            let end_info = condition_context.get_resolved_info(&end_info)?;
            let mut last_line_number = start_info.line_number;
            let mut last_allows_multi_line = true;
            let mut last_allows_single_line = false;
            let mut has_multi_line_value = false;
            let value_datas = value_datas.borrow();
            for (i, value_data) in value_datas.iter().enumerate() {
                // ignore, it will always be at the start of the line
                if i == 0 && is_start_standalone_line { continue; }

                let value_start_info = condition_context.get_resolved_info(&value_data.start_info)?;
                // check if any of the value starts are at the beginning of the line
                if value_start_info.is_start_of_line() { return Some(true); }

                let last_is_multi_line_value = last_line_number < value_start_info.line_number;
                if i == 1 && last_is_multi_line_value {
                    has_multi_line_value = true;
                }

                if i >= 1 && check_value_should_make_multi_line(
                    last_is_multi_line_value,
                    last_allows_multi_line,
                    last_allows_single_line,
                    has_multi_line_value,
                ) {
                    return Some(true);
                }

                last_line_number = value_start_info.line_number;
                last_allows_multi_line = value_data.allow_inline_multi_line;
                last_allows_single_line = value_data.allow_inline_single_line;
            }

            // check if the last node is single-line
            let last_is_multi_line_value = last_line_number < end_info.line_number;
            Some(check_value_should_make_multi_line(
                last_is_multi_line_value,
                last_allows_multi_line,
                last_allows_single_line,
                has_multi_line_value,
            ))
        }),
        false_path: None,
        true_path: None,
    }, vec![end_info]);

    fn check_value_should_make_multi_line(
        is_multi_line_value: bool,
        allows_multi_line: bool,
        allows_single_line: bool,
        has_multi_line_value: bool,
    ) -> bool {
        if is_multi_line_value {
            !allows_multi_line
        } else {
            has_multi_line_value && !allows_single_line
        }
    }
}

fn get_spans_multiple_lines(value_datas: Rc<RefCell<Vec<ParsedValueData>>>, end_info: Info) -> Condition {
    Condition::new_with_dependent_infos("spansMultipleLines", ConditionProperties {
        condition: Box::new(move |condition_context| {
            if let Some(first_value_data) = value_datas.borrow().first() {
                condition_resolvers::is_multiple_lines(condition_context, &first_value_data.start_info, &end_info)
            } else {
                Some(false)
            }
        }),
        false_path: None,
        true_path: None,
    }, vec![end_info])
}

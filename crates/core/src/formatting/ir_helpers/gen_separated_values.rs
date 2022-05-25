use std::cell::RefCell;
use std::rc::Rc;

use crate::formatting::condition_helpers;

use super::super::condition_resolvers;
use super::super::conditions::*;
use super::super::ir_helpers;
use super::super::print_items::*;

pub struct GenSeparatedValuesOptions {
  pub prefer_hanging: bool,
  pub force_use_new_lines: bool,
  pub allow_blank_lines: bool,
  pub single_line_space_at_start: bool,
  pub single_line_space_at_end: bool,
  pub single_line_separator: PrintItems,
  pub indent_width: u8,
  pub multi_line_options: MultiLineOptions,
  /// Forces a possible newline at the start when there are values.
  /// If this isn't used, then a possible newline won't happen when
  /// the value is below the line
  pub force_possible_newline_at_start: bool,
}

pub enum BoolOrCondition {
  Bool(bool),
  Condition(ConditionResolver),
}

pub struct MultiLineOptions {
  pub newline_at_start: bool,
  pub newline_at_end: bool,
  pub with_indent: bool,
  pub with_hanging_indent: BoolOrCondition,
  pub maintain_line_breaks: bool,
}

impl MultiLineOptions {
  pub fn new_line_start() -> Self {
    // todo: rename: newlinestarewithindent
    MultiLineOptions {
      newline_at_start: true,
      newline_at_end: false,
      with_indent: true,
      with_hanging_indent: BoolOrCondition::Bool(false),
      maintain_line_breaks: false,
    }
  }

  pub fn surround_newlines_indented() -> Self {
    MultiLineOptions {
      newline_at_start: true,
      newline_at_end: true,
      with_indent: true,
      with_hanging_indent: BoolOrCondition::Bool(false),
      maintain_line_breaks: false,
    }
  }

  pub fn same_line_start_hanging_indent() -> Self {
    MultiLineOptions {
      newline_at_start: false,
      newline_at_end: false,
      with_indent: false,
      with_hanging_indent: BoolOrCondition::Bool(true),
      maintain_line_breaks: false,
    }
  }

  pub fn same_line_no_indent() -> Self {
    MultiLineOptions {
      newline_at_start: false,
      newline_at_end: false,
      with_indent: false,
      with_hanging_indent: BoolOrCondition::Bool(false),
      maintain_line_breaks: false,
    }
  }

  pub fn maintain_line_breaks() -> Self {
    MultiLineOptions {
      newline_at_start: false,
      newline_at_end: false,
      with_indent: false,
      with_hanging_indent: BoolOrCondition::Bool(false),
      maintain_line_breaks: true,
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct LinesSpan {
  pub start_line: usize,
  pub end_line: usize,
}

pub struct GeneratedValue {
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

impl GeneratedValue {
  /// Use this when you don't care about blank lines.
  pub fn from_items(items: PrintItems) -> GeneratedValue {
    GeneratedValue {
      items,
      lines_span: None,
      allow_inline_multi_line: false,
      allow_inline_single_line: false,
    }
  }
}

pub struct GenSeparatedValuesResult {
  pub items: PrintItems,
  pub is_multi_line_condition_ref: ConditionReference,
}

struct GeneratedValueData {
  is_start_of_line: IsStartOfLine,
  line_number: LineNumber,
  line_start_indent_level: LineStartIndentLevel,
  allow_inline_multi_line: bool,
  allow_inline_single_line: bool,
}

pub fn gen_separated_values(
  generated_values: impl FnOnce(&ConditionReference) -> Vec<GeneratedValue>,
  opts: GenSeparatedValuesOptions,
) -> GenSeparatedValuesResult {
  let indent_width = opts.indent_width;
  let start_ln = LineNumber::new("startSeparatedValues");
  let start_cn = ColumnNumber::new("startSeparatedValues");
  let start_lscn = LineStartColumnNumber::new("startSeparatedValues");
  let end_ln = LineNumber::new("endSeparatedValues");
  let value_datas: Rc<RefCell<Vec<GeneratedValueData>>> = Rc::new(RefCell::new(Vec::new()));
  let multi_line_options = opts.multi_line_options;
  let mut is_start_standalone_line = get_is_start_standalone_line(start_cn, start_lscn);
  let is_start_standalone_line_ref = is_start_standalone_line.create_reference();
  let mut is_multi_line_condition = {
    if opts.force_use_new_lines {
      Condition::new_true()
    } else if opts.prefer_hanging {
      if !multi_line_options.newline_at_start {
        Condition::new_false()
      } else {
        get_is_multi_line_for_hanging(value_datas.clone(), is_start_standalone_line_ref)
      }
    } else {
      get_is_multi_line_for_multi_line(start_ln, value_datas.clone(), is_start_standalone_line_ref, end_ln)
    }
  };
  let is_multi_line_reevaluation = is_multi_line_condition.create_reevaluation();
  let is_multi_line_condition_ref = is_multi_line_condition.create_reference();
  let is_multi_line = is_multi_line_condition_ref.create_resolver();

  let mut items = PrintItems::new();
  items.push_info(start_cn);
  items.push_info(start_lscn);
  items.push_info(start_ln);
  items.extend(clear_resolutions_on_position_change(value_datas.clone(), end_ln));
  items.push_condition(is_start_standalone_line);
  items.push_condition(is_multi_line_condition);

  let generated_values = (generated_values)(
    &is_multi_line_condition_ref, // need to use a sized value it seems...
  );
  let has_values = !generated_values.is_empty();
  let inner_gen_result = inner_gen(
    generated_values,
    is_multi_line.clone(),
    opts.single_line_separator,
    &multi_line_options,
    opts.allow_blank_lines,
  );
  value_datas.borrow_mut().extend(inner_gen_result.value_datas);
  let generated_values_items = inner_gen_result.items.into_rc_path();
  items.push_condition(Condition::new(
    "multiLineOrHanging",
    ConditionProperties {
      condition: is_multi_line,
      true_path: Some(
        if_true_or(
          "newLineIndentedIfNotStandalone",
          Rc::new(move |context| Some(!context.resolved_condition(&is_start_standalone_line_ref)?)),
          {
            let mut items = PrintItems::new();
            if multi_line_options.newline_at_start {
              items.push_signal(Signal::NewLine);
            }
            if multi_line_options.with_indent {
              items.push_signal(Signal::StartIndent);
            }
            items.extend(generated_values_items.into());
            if multi_line_options.with_indent {
              items.push_signal(Signal::FinishIndent);
            }
            if multi_line_options.newline_at_end {
              items.push_signal(Signal::NewLine);
            }
            items
          },
          generated_values_items.into(),
        )
        .into(),
      ),
      false_path: Some({
        let mut items = PrintItems::new();
        let has_start_space = opts.single_line_space_at_start;
        if has_start_space {
          items.push_signal(Signal::SpaceIfNotTrailing);
          items.push_signal(Signal::PossibleNewLine);
        }
        if has_values && multi_line_options.newline_at_start {
          // place this after the space so the first item will start on a newline when there is a newline here
          items.push_condition(if_above_width(
            if opts.force_possible_newline_at_start {
              0
            } else {
              indent_width + if has_start_space { 1 } else { 0 }
            },
            Signal::PossibleNewLine.into(),
          ));
        }
        items.extend(generated_values_items.into());
        if opts.single_line_space_at_end {
          items.push_str(" ");
        }
        items
      }),
    },
  ));

  items.push_info(end_ln);
  items.push_reevaluation(is_multi_line_reevaluation);

  return GenSeparatedValuesResult {
    items,
    is_multi_line_condition_ref,
  };

  struct InnerGenResult {
    items: PrintItems,
    value_datas: Vec<GeneratedValueData>,
  }

  fn inner_gen(
    generated_values: Vec<GeneratedValue>,
    is_multi_line: ConditionResolver,
    single_line_separator: PrintItems,
    multi_line_options: &MultiLineOptions,
    allow_blank_lines: bool,
  ) -> InnerGenResult {
    let mut items = PrintItems::new();
    let mut value_datas = Vec::new();
    let values_count = generated_values.len();
    let single_line_separator = single_line_separator.into_rc_path();
    let mut last_lines_span: Option<LinesSpan> = None;
    let maintain_line_breaks = multi_line_options.maintain_line_breaks;
    let mut had_newline = false;
    let first_line_number = LineNumber::new("firstValue");
    let mut last_line_start_indent_level = None;

    for (i, generated_value) in generated_values.into_iter().enumerate() {
      let start_line_number = if i == 0 { first_line_number } else { LineNumber::new("value") };
      let start_is_start_of_line = IsStartOfLine::new("value");
      let start_line_start_indent_level = LineStartIndentLevel::new("value");
      value_datas.push(GeneratedValueData {
        line_number: start_line_number,
        is_start_of_line: start_is_start_of_line,
        line_start_indent_level: start_line_start_indent_level,
        allow_inline_multi_line: generated_value.allow_inline_multi_line,
        allow_inline_single_line: generated_value.allow_inline_single_line,
      });

      if i == 0 {
        if multi_line_options.newline_at_start && values_count > 1 {
          items.push_condition(if_false(
            "isNotStartOfLine",
            condition_resolvers::is_start_of_line(),
            Signal::PossibleNewLine.into(),
          ));
        }

        items.push_info(start_line_number);
        items.push_info(start_is_start_of_line);
        items.push_info(start_line_start_indent_level);
        items.extend(generated_value.items);
      } else {
        let (has_new_line, has_blank_line) = if let Some(last_lines_span) = last_lines_span {
          if let Some(current_lines_span) = generated_value.lines_span {
            (
              last_lines_span.end_line < current_lines_span.start_line,
              last_lines_span.end_line < std::cmp::max(current_lines_span.start_line, 1) - 1, // prevent subtracting with overflow
            )
          } else {
            (false, false)
          }
        } else {
          (false, false)
        };
        let use_blank_line = allow_blank_lines && has_blank_line;
        let generated_value = generated_value.items.into_rc_path();
        items.push_condition(Condition::new(
          "multiLineOrHangingCondition",
          ConditionProperties {
            condition: is_multi_line.clone(),
            true_path: {
              let mut items = PrintItems::new();
              if use_blank_line {
                items.push_signal(Signal::NewLine);
              }
              if !maintain_line_breaks || has_new_line {
                items.push_signal(Signal::NewLine);
                had_newline = true;
              } else {
                let space_or_newline = {
                  if let Some(last_line_start_indent_level) = last_line_start_indent_level {
                    if_true_or(
                      "newlineIfHanging",
                      Rc::new(move |context| condition_helpers::is_hanging(context, last_line_start_indent_level, None)),
                      Signal::NewLine.into(),
                      single_line_separator.into(),
                    )
                    .into()
                  } else {
                    single_line_separator.into()
                  }
                };
                if i == values_count - 1 && !had_newline {
                  // If there hasn't been a newline, then this should be forced to be one
                  // since this is in multi-line mode (meaning, since we're here, one of these
                  // was a newline due to the line width so it must be this one)
                  items.push_condition(if_true_or(
                    "forcedNewLineIfNoNewLine",
                    Rc::new(move |context| condition_helpers::is_on_different_line(context, first_line_number)),
                    space_or_newline,
                    Signal::NewLine.into(),
                  ))
                } else {
                  items.extend(space_or_newline);
                }
              }

              match &multi_line_options.with_hanging_indent {
                BoolOrCondition::Bool(with_hanging_indent) => {
                  if *with_hanging_indent {
                    items.push_condition(indent_if_start_of_line({
                      let mut items = PrintItems::new();
                      items.push_info(start_line_number);
                      items.push_info(start_is_start_of_line);
                      items.push_info(start_line_start_indent_level);
                      items.extend(generated_value.into());
                      items
                    }));
                  } else {
                    items.push_info(start_line_number);
                    items.push_info(start_is_start_of_line);
                    items.push_info(start_line_start_indent_level);
                    items.extend(generated_value.into());
                  }
                }
                BoolOrCondition::Condition(condition) => {
                  let inner_items = {
                    let mut items = PrintItems::new();
                    items.push_info(start_line_number);
                    items.push_info(start_is_start_of_line);
                    items.push_info(start_line_start_indent_level);
                    items.extend(generated_value.into());
                    items
                  }
                  .into_rc_path();
                  items.push_condition(Condition::new(
                    "valueHangingIndent",
                    ConditionProperties {
                      condition: condition.clone(),
                      true_path: Some(ir_helpers::with_indent(inner_items.into())),
                      false_path: Some(inner_items.into()),
                    },
                  ));
                }
              }

              Some(items)
            },
            false_path: {
              let mut items = PrintItems::new();
              items.extend(single_line_separator.into()); // ex. Signal::SpaceOrNewLine
              items.push_condition(indent_if_start_of_line({
                let mut items = PrintItems::new();
                items.push_info(start_line_number);
                items.push_info(start_is_start_of_line);
                items.push_info(start_line_start_indent_level);
                items.extend(generated_value.into());
                items
              }));
              Some(items)
            },
          },
        ));
      }

      last_lines_span = generated_value.lines_span;
      last_line_start_indent_level.replace(start_line_start_indent_level);
    }

    InnerGenResult { items, value_datas }
  }
}

fn clear_resolutions_on_position_change(value_datas: Rc<RefCell<Vec<GeneratedValueData>>>, end_line_number: LineNumber) -> PrintItems {
  let mut items = PrintItems::new();
  let column_number = ColumnNumber::new("clearer");
  // todo: use anchors instead of taking line number into account here
  let line_number = LineNumber::new("clearer");
  items.push_condition(Condition::new(
    "clearWhenPositionChanges",
    ConditionProperties {
      condition: Rc::new(move |condition_context| {
        let column_number = condition_context.resolved_column_number(column_number)?;
        let line_number = condition_context.resolved_line_number(line_number)?;
        // when the position changes, clear all the infos so they get re-evaluated again
        if column_number != condition_context.writer_info.column_number || line_number != condition_context.writer_info.line_number {
          for value_data in value_datas.borrow().iter() {
            condition_context.clear_info(value_data.line_number);
            condition_context.clear_info(value_data.is_start_of_line);
            condition_context.clear_info(value_data.line_start_indent_level);
          }
          condition_context.clear_info(end_line_number);
        }

        None
      }),
      false_path: None,
      true_path: None,
    },
  ));
  items.push_info(line_number);
  items.push_info(column_number);
  items
}

fn get_is_start_standalone_line(start_cn: ColumnNumber, start_lscn: LineStartColumnNumber) -> Condition {
  Condition::new(
    "isStartStandaloneLine",
    ConditionProperties {
      condition: Rc::new(move |condition_context| {
        let start_cn = condition_context.resolved_column_number(start_cn);
        let start_lscn = condition_context.resolved_line_start_column_number(start_lscn);
        let is_column_number_at_line_start = start_cn? == start_lscn?;
        Some(is_column_number_at_line_start)
      }),
      false_path: None,
      true_path: None,
    },
  )
}

fn get_is_multi_line_for_hanging(value_datas: Rc<RefCell<Vec<GeneratedValueData>>>, is_start_standalone_line_ref: ConditionReference) -> Condition {
  Condition::new(
    "isMultiLineForHanging",
    ConditionProperties {
      condition: Rc::new(move |condition_context| {
        let is_start_standalone_line = condition_context.resolved_condition(&is_start_standalone_line_ref)?;
        if is_start_standalone_line {
          // check if the second value is on a newline
          if let Some(second_value_data) = value_datas.borrow().iter().nth(1) {
            return condition_context.resolved_is_start_of_line(second_value_data.is_start_of_line);
          }
        } else {
          // check if the first value is at the beginning of the line
          if let Some(first_value_data) = value_datas.borrow().iter().next() {
            return condition_context.resolved_is_start_of_line(first_value_data.is_start_of_line);
          }
        }

        Some(false)
      }),
      false_path: None,
      true_path: None,
    },
  )
}

fn get_is_multi_line_for_multi_line(
  start_ln: LineNumber,
  value_datas: Rc<RefCell<Vec<GeneratedValueData>>>,
  is_start_standalone_line_ref: ConditionReference,
  end_ln: LineNumber,
) -> Condition {
  let last_result = Rc::new(RefCell::new(false));
  return Condition::new(
    "isMultiLineForMultiLine",
    ConditionProperties {
      condition: Rc::new(move |condition_context| {
        let result = evaluate(start_ln, &value_datas, &is_start_standalone_line_ref, end_ln, condition_context);
        let mut last_result = last_result.borrow_mut();
        // If the last result was ever true and this result is `Some(false)`,
        // that means something trailing on the last line is causing it the
        // condition to be multi-line and we shouldn't revert back to not being
        // multi-line which would cause an infinite loop. Additionally, we know
        // that the start position hasn't changed since it's not `None` where
        // the infos have been cleared on position change.
        // See https://github.com/dprint/dprint-plugin-typescript/issues/372 for more details
        if *last_result && matches!(result, Some(false)) {
          return Some(true);
        }
        *last_result = result.unwrap_or(false);
        result
      }),
      false_path: None,
      true_path: None,
    },
  );

  fn evaluate(
    start_ln: LineNumber,
    value_datas: &Rc<RefCell<Vec<GeneratedValueData>>>,
    is_start_standalone_line_ref: &ConditionReference,
    end_ln: LineNumber,
    condition_context: &mut ConditionResolverContext,
  ) -> Option<bool> {
    // todo: This is slightly confusing because it works on the "last" value rather than the current
    let is_start_standalone_line = condition_context.resolved_condition(is_start_standalone_line_ref)?;
    let start_ln = condition_context.resolved_line_number(start_ln)?;
    let end_ln = condition_context.resolved_line_number(end_ln)?;
    let mut last_ln = start_ln;
    let mut last_allows_multi_line = true;
    let mut last_allows_single_line = false;
    let mut has_multi_line_value = false;
    let value_datas = value_datas.borrow();

    for (i, value_data) in value_datas.iter().enumerate() {
      // ignore, it will always be at the start of the line
      if i == 0 && is_start_standalone_line {
        continue;
      }

      let value_start_is_start_of_line = condition_context.resolved_is_start_of_line(value_data.is_start_of_line)?;
      // check if any of the value starts are at the beginning of the line
      if value_start_is_start_of_line {
        return Some(true);
      }
      let value_start_ln = condition_context.resolved_line_number(value_data.line_number)?;

      if i >= 1 {
        // todo: consolidate with below
        let last_is_multi_line_value = last_ln < value_start_ln;
        if last_is_multi_line_value {
          has_multi_line_value = true;
        }

        if check_value_should_make_multi_line(last_is_multi_line_value, last_allows_multi_line, last_allows_single_line, has_multi_line_value) {
          return Some(true);
        }
      }

      last_ln = value_start_ln;
      last_allows_multi_line = value_data.allow_inline_multi_line;
      last_allows_single_line = value_data.allow_inline_single_line;
    }

    // check if the last node is single-line
    // todo: consolidate with above
    let last_is_multi_line_value = last_ln < end_ln;
    if last_is_multi_line_value {
      has_multi_line_value = true;
    }
    Some(check_value_should_make_multi_line(
      last_is_multi_line_value,
      last_allows_multi_line,
      last_allows_single_line,
      has_multi_line_value,
    ))
  }

  fn check_value_should_make_multi_line(is_multi_line_value: bool, allows_multi_line: bool, allows_single_line: bool, has_multi_line_value: bool) -> bool {
    if is_multi_line_value {
      !allows_multi_line
    } else {
      has_multi_line_value && !allows_single_line
    }
  }
}

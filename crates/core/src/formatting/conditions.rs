use std::rc::Rc;

use super::print_items::*;
use super::*;

pub fn indent_if_start_of_line(items: PrintItems) -> Condition {
  let rc_path = items.into_rc_path();
  if_true_or(
    "indentIfStartOfLine",
    condition_resolvers::is_start_of_line(),
    ir_helpers::with_indent(rc_path.into()),
    rc_path.into(),
  )
}

pub fn indent_if_start_of_line_or_start_of_line_indented(items: PrintItems) -> Condition {
  let rc_path = items.into_rc_path();
  conditions::if_true_or(
    "withIndentIfStartOfLineOrStartOfLineIndented",
    condition_resolvers::is_start_of_line_or_is_start_of_line_indented(),
    ir_helpers::with_indent(rc_path.into()),
    rc_path.into(),
  )
}

pub fn with_indent_if_start_of_line_indented(items: PrintItems) -> Condition {
  let rc_path = items.into_rc_path();
  if_true_or(
    "withIndentIfStartOfLineIndented",
    condition_resolvers::is_start_of_line_indented(),
    ir_helpers::with_indent(rc_path.into()),
    rc_path.into(),
  )
}

pub struct NewLineIfHangingSpaceOtherwiseOptions {
  pub start_info: Info,
  pub end_info: Option<Info>,
  pub space_char: Option<PrintItems>,
}

pub fn new_line_if_hanging_space_otherwise(opts: NewLineIfHangingSpaceOtherwiseOptions) -> Condition {
  let space_char = opts.space_char.unwrap_or_else(|| " ".into());
  let start_info = opts.start_info;
  let end_info = opts.end_info;

  if_true_or(
    "newLineIfHangingSpaceOtherwise",
    Rc::new(move |context| condition_helpers::is_hanging(context, &start_info, &end_info)),
    Signal::NewLine.into(),
    space_char,
  )
}

pub fn space_if_not_start_of_line(space_char: Option<PrintItems>) -> Condition {
  let space_char = space_char.unwrap_or_else(|| " ".into());

  if_true(
    "spaceIfNotStartOfLine",
    move |context| Some(!context.writer_info.is_start_of_line()),
    space_char,
  )
}

pub fn new_line_if_hanging(start_info: Info, end_info: Option<Info>) -> Condition {
  if_true(
    "newlineIfHanging",
    Rc::new(move |context| condition_helpers::is_hanging(context, &start_info, &end_info)),
    Signal::NewLine.into(),
  )
}

/// This condition can be used to force the printer to jump back to the point
/// this condition exists at once the provided info is resolved.
pub fn force_reevaluation_once_resolved(info: Info) -> Condition {
  Condition::new(
    "forceReevaluationOnceInfoResolved",
    ConditionProperties {
      condition: Rc::new(move |context| {
        let resolved_info = context.get_resolved_info(&info);
        if resolved_info.is_some() {
          Some(false)
        } else {
          None
        }
      }),
      true_path: None,
      false_path: None,
    },
  )
}

pub fn new_line_if_multiple_lines_space_or_new_line_otherwise(start_info: Info, end_info: Option<Info>) -> Condition {
  if_true_or(
    "newLineIfMultipleLinesSpaceOrNewLineOtherwise",
    Rc::new(move |context| {
      let start_info = context.get_resolved_info(&start_info)?;
      let end_info = {
        if let Some(end_info) = &end_info {
          context.get_resolved_info(end_info)?
        } else {
          &context.writer_info
        }
      };

      Some(end_info.line_number > start_info.line_number)
    }),
    Signal::NewLine.into(),
    Signal::SpaceOrNewLine.into(),
  )
}

pub fn single_indent_if_start_of_line() -> Condition {
  if_true(
    "singleIndentIfStartOfLine",
    condition_resolvers::is_start_of_line(),
    Signal::SingleIndent.into(),
  )
}

/// Prints the provided items when the current relative column number is above
/// the specified width.
pub fn if_above_width(width: u8, items: PrintItems) -> Condition {
  if_above_width_or(width, items, PrintItems::new())
}

/// Prints the provided true_items when the current relative column number is above
/// the specified width or prints the false_items otherwise.
pub fn if_above_width_or(width: u8, true_items: PrintItems, false_items: PrintItems) -> Condition {
  Condition::new(
    "ifAboveWidth",
    ConditionProperties {
      condition: Rc::new(move |context| {
        let writer_info = &context.writer_info;
        let first_indent_col = writer_info.line_start_column_number() + (width as u32);
        Some(writer_info.column_number > first_indent_col)
      }),
      true_path: Some(true_items),
      false_path: if false_items.is_empty() { None } else { Some(false_items) },
    },
  )
}

pub fn if_true(name: &'static str, resolver: ConditionResolver, true_path: PrintItems) -> Condition {
  Condition::new(
    name,
    ConditionProperties {
      true_path: Some(true_path),
      false_path: None,
      condition: resolver,
    },
  )
}

pub fn if_true_or(name: &'static str, resolver: ConditionResolver, true_path: PrintItems, false_path: PrintItems) -> Condition {
  Condition::new(
    name,
    ConditionProperties {
      true_path: Some(true_path),
      false_path: Some(false_path),
      condition: resolver,
    },
  )
}

pub fn if_false(name: &'static str, resolver: ConditionResolver, false_path: PrintItems) -> Condition {
  Condition::new(
    name,
    ConditionProperties {
      true_path: None,
      false_path: Some(false_path),
      condition: resolver,
    },
  )
}

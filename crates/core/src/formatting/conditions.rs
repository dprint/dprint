use std::rc::Rc;

use super::print_items::*;
use super::*;

pub fn indent_if_start_of_line(items: PrintItems<'_>) -> Condition<'_> {
  let arena = items.arena();
  let rc_path = items.into_rc_path();
  if_true_or(
    "indentIfStartOfLine",
    condition_resolvers::is_start_of_line(),
    ir_helpers::with_indent(rc_path.into_print_items(arena)),
    rc_path.into_print_items(arena),
  )
}

pub fn indent_if_start_of_line_or_start_of_line_indented(items: PrintItems<'_>) -> Condition<'_> {
  let arena = items.arena();
  let rc_path = items.into_rc_path();
  conditions::if_true_or(
    "withIndentIfStartOfLineOrStartOfLineIndented",
    condition_resolvers::is_start_of_line_or_is_start_of_line_indented(),
    ir_helpers::with_indent(rc_path.into_print_items(arena)),
    rc_path.into_print_items(arena),
  )
}

pub fn with_indent_if_start_of_line_indented(items: PrintItems<'_>) -> Condition<'_> {
  let arena = items.arena();
  let rc_path = items.into_rc_path();
  if_true_or(
    "withIndentIfStartOfLineIndented",
    condition_resolvers::is_start_of_line_indented(),
    ir_helpers::with_indent(rc_path.into_print_items(arena)),
    rc_path.into_print_items(arena),
  )
}

pub struct NewLineIfHangingSpaceOtherwiseOptions<'a> {
  pub start_lsil: LineStartIndentLevel,
  pub end_lsil: Option<LineStartIndentLevel>,
  pub space_char: Option<PrintItems<'a>>,
}

pub fn new_line_if_hanging_space_otherwise<'a>(arena: &'a Arena, opts: NewLineIfHangingSpaceOtherwiseOptions<'a>) -> Condition<'a> {
  let space_char = opts.space_char.unwrap_or_else(|| " ".into_print_items(arena));
  let start_lsil = opts.start_lsil;
  let end_lsil = opts.end_lsil;

  if_true_or(
    "newLineIfHangingSpaceOtherwise",
    Rc::new(move |context| condition_helpers::is_hanging(context, start_lsil, end_lsil)),
    Signal::NewLine.into_print_items(arena),
    space_char,
  )
}

pub fn new_line_if_hanging(arena: &Arena, start_lsil: LineStartIndentLevel, end_lsil: Option<LineStartIndentLevel>) -> Condition<'_> {
  if_true(
    "newlineIfHanging",
    Rc::new(move |context| condition_helpers::is_hanging(context, start_lsil, end_lsil)),
    Signal::NewLine.into_print_items(arena),
  )
}

pub fn new_line_if_multiple_lines_space_or_new_line_otherwise(arena: &Arena, start_ln: LineNumber, end_ln: Option<LineNumber>) -> Condition<'_> {
  if_true_or(
    "newLineIfMultipleLinesSpaceOrNewLineOtherwise",
    Rc::new(move |context| {
      let start_ln = context.resolved_line_number(start_ln)?;
      let end_ln = {
        if let Some(end_ln) = end_ln {
          context.resolved_line_number(end_ln)?
        } else {
          context.writer_info.line_number
        }
      };

      Some(end_ln > start_ln)
    }),
    Signal::NewLine.into_print_items(arena),
    Signal::SpaceOrNewLine.into_print_items(arena),
  )
}

pub fn single_indent_if_start_of_line(arena: &Arena) -> Condition<'_> {
  if_true(
    "singleIndentIfStartOfLine",
    condition_resolvers::is_start_of_line(),
    Signal::SingleIndent.into_print_items(arena),
  )
}

/// Prints the provided items when the current relative column number is above
/// the specified width.
pub fn if_above_width(width: u8, items: PrintItems<'_>) -> Condition<'_> {
  let arena = items.arena();
  if_above_width_or(width, items, PrintItems::new(arena))
}

/// Prints the provided true_items when the current relative column number is above
/// the specified width or prints the false_items otherwise.
pub fn if_above_width_or<'a>(width: u8, true_items: PrintItems<'a>, false_items: PrintItems<'a>) -> Condition<'a> {
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

pub fn if_true<'a>(name: &'static str, resolver: ConditionResolver, true_path: PrintItems<'a>) -> Condition<'a> {
  Condition::new(
    name,
    ConditionProperties {
      true_path: Some(true_path),
      false_path: None,
      condition: resolver,
    },
  )
}

pub fn if_true_or<'a>(name: &'static str, resolver: ConditionResolver, true_path: PrintItems<'a>, false_path: PrintItems<'a>) -> Condition<'a> {
  Condition::new(
    name,
    ConditionProperties {
      true_path: Some(true_path),
      false_path: Some(false_path),
      condition: resolver,
    },
  )
}

pub fn if_false<'a>(name: &'static str, resolver: ConditionResolver, false_path: PrintItems<'a>) -> Condition<'a> {
  Condition::new(
    name,
    ConditionProperties {
      true_path: None,
      false_path: Some(false_path),
      condition: resolver,
    },
  )
}

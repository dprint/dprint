use super::ConditionResolverContext;
use super::LineAndColumn;
use super::LineNumber;
use super::LineStartIndentLevel;

pub fn is_multiple_lines(condition_context: &mut ConditionResolverContext, start_ln: LineNumber, end_ln: LineNumber) -> Option<bool> {
  let start_ln = condition_context.get_resolved_line_number(start_ln)?;
  let end_ln = condition_context.get_resolved_line_number(end_ln)?;

  Some(end_ln > start_ln)
}

pub fn is_hanging(condition_context: &mut ConditionResolverContext, start_lsil: LineStartIndentLevel, end_lsil: Option<LineStartIndentLevel>) -> Option<bool> {
  let start_indent_level = condition_context.get_resolved_line_start_indent_level(start_lsil)?;
  let end_indent_level = get_resolved_end_lsil(condition_context, end_lsil)?;
  return Some(end_indent_level > start_indent_level);

  fn get_resolved_end_lsil<'a>(condition_context: &'a ConditionResolverContext, end_lsil: Option<LineStartIndentLevel>) -> Option<u8> {
    if let Some(end_lsil) = end_lsil {
      condition_context.get_resolved_line_start_indent_level(end_lsil)
    } else {
      // use the current condition position
      Some(condition_context.writer_info.line_start_indent_level)
    }
  }
}

pub fn are_line_and_columns_not_equal(condition_context: &mut ConditionResolverContext, start_lc: LineAndColumn, end_lc: LineAndColumn) -> Option<bool> {
  let are_equal = are_line_and_columns_equal(condition_context, start_lc, end_lc)?;
  Some(!are_equal)
}

pub fn are_line_and_columns_equal(condition_context: &mut ConditionResolverContext, start_lc: LineAndColumn, end_lc: LineAndColumn) -> Option<bool> {
  let start_lc = condition_context.get_resolved_line_and_column(start_lc)?;
  let end_lc = condition_context.get_resolved_line_and_column(end_lc)?;
  Some(start_lc == end_lc)
}

pub fn is_at_same_position(condition_context: &mut ConditionResolverContext, line_and_col: LineAndColumn) -> Option<bool> {
  let (start_ln, start_col) = condition_context.get_resolved_line_and_column(line_and_col)?;
  Some(start_ln == condition_context.writer_info.line_number && start_col == condition_context.writer_info.column_number)
}

pub fn is_on_same_line(condition_context: &mut ConditionResolverContext, start_ln: LineNumber) -> Option<bool> {
  let start_ln = condition_context.get_resolved_line_number(start_ln)?;
  Some(start_ln == condition_context.writer_info.line_number)
}

pub fn is_on_different_line(condition_context: &mut ConditionResolverContext, line_number: LineNumber) -> Option<bool> {
  let line_number = condition_context.get_resolved_line_number(line_number)?;
  Some(line_number != condition_context.writer_info.line_number)
}

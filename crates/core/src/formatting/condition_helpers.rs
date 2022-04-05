use crate::formatting::WriterInfo;

use super::ColumnNumber;
use super::ConditionResolverContext;
use super::Info;
use super::LineAndColumn;
use super::LineNumber;
use super::LineStartIndentLevel;

pub fn is_multiple_lines_delete(condition_context: &mut ConditionResolverContext, start_info: &Info, end_info: &Info) -> Option<bool> {
  let start_info = condition_context.get_resolved_info(start_info)?;
  let end_info = condition_context.get_resolved_info(end_info)?;

  Some(end_info.line_number > start_info.line_number)
}

pub fn is_multiple_lines(condition_context: &mut ConditionResolverContext, start_ln: LineNumber, end_ln: LineNumber) -> Option<bool> {
  let start_ln = condition_context.get_resolved_line_number(start_ln)?;
  let end_ln = condition_context.get_resolved_line_number(end_ln)?;

  Some(end_ln > start_ln)
}

pub fn is_hanging(condition_context: &mut ConditionResolverContext, start_lsil: LineStartIndentLevel, end_lsil: Option<LineStartIndentLevel>) -> Option<bool> {
  let start_indent_level = condition_context.get_resolved_line_start_indent_level(start_lsil)?;
  let end_indent_level = get_resolved_end_info(condition_context, end_lsil)?;
  return Some(end_indent_level > start_indent_level);

  fn get_resolved_end_info<'a>(condition_context: &'a ConditionResolverContext, end_info: Option<LineStartIndentLevel>) -> Option<u8> {
    if let Some(end_info) = end_info {
      condition_context.get_resolved_line_start_indent_level(end_info)
    } else {
      // use the current condition position
      Some(condition_context.writer_info.line_start_indent_level)
    }
  }
}

pub fn is_hanging_delete(condition_context: &mut ConditionResolverContext, start_info: &Info, end_info: &Option<Info>) -> Option<bool> {
  let resolved_start_info = condition_context.get_resolved_info(start_info)?;
  let resolved_end_info = get_resolved_end_info(condition_context, end_info)?;
  return Some(resolved_end_info.line_start_indent_level > resolved_start_info.line_start_indent_level);

  fn get_resolved_end_info<'a>(condition_context: &'a ConditionResolverContext, end_info: &Option<Info>) -> Option<&'a WriterInfo> {
    if let Some(end_info) = end_info {
      condition_context.get_resolved_info(end_info)
    } else {
      // use the current condition position
      Some(&condition_context.writer_info)
    }
  }
}

pub fn are_infos_not_equal(condition_context: &mut ConditionResolverContext, start_info: &Info, end_info: &Info) -> Option<bool> {
  let are_equal = are_infos_equal(condition_context, start_info, end_info)?;
  Some(!are_equal)
}

pub fn are_infos_equal(condition_context: &mut ConditionResolverContext, start_info: &Info, end_info: &Info) -> Option<bool> {
  let start_info = condition_context.get_resolved_info(start_info)?;
  let end_info = condition_context.get_resolved_info(end_info)?;
  Some(start_info.line_number == end_info.line_number && start_info.column_number == end_info.column_number)
}

pub fn is_at_same_position(condition_context: &mut ConditionResolverContext, line_and_col: LineAndColumn) -> Option<bool> {
  let (start_ln, start_col) = condition_context.get_resolved_line_and_column(line_and_col)?;
  Some(start_ln == condition_context.writer_info.line_number && start_col == condition_context.writer_info.column_number)
}

pub fn is_at_same_position_delete(condition_context: &mut ConditionResolverContext, start_info: &Info) -> Option<bool> {
  let start_info = condition_context.get_resolved_info(start_info)?;
  Some(start_info.line_number == condition_context.writer_info.line_number && start_info.column_number == condition_context.writer_info.column_number)
}

pub fn is_on_same_line(condition_context: &mut ConditionResolverContext, start_info: &Info) -> Option<bool> {
  let start_info = condition_context.get_resolved_info(start_info)?;
  Some(start_info.line_number == condition_context.writer_info.line_number)
}

pub fn is_on_different_line_delete(condition_context: &mut ConditionResolverContext, start_info: &Info) -> Option<bool> {
  let start_info = condition_context.get_resolved_info(start_info)?;
  Some(start_info.line_number != condition_context.writer_info.line_number)
}

pub fn is_on_different_line(condition_context: &mut ConditionResolverContext, line_number: LineNumber) -> Option<bool> {
  let line_number = condition_context.get_resolved_line_number(line_number)?;
  Some(line_number != condition_context.writer_info.line_number)
}

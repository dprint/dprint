use super::print_items::*;

pub fn is_start_of_line(condition_context: &ConditionResolverContext) -> bool {
    condition_context.writer_info.is_start_of_line()
}

pub fn is_multiple_lines(condition_context: &mut ConditionResolverContext, start_info: &Info, end_info: &Info) -> Option<bool> {
    let start_info = condition_context.get_resolved_info(start_info)?;
    let end_info = condition_context.get_resolved_info(end_info)?;

    Some(end_info.line_number > start_info.line_number)
}

pub fn is_hanging(condition_context: &mut ConditionResolverContext, start_info: &Info, end_info: &Option<Info>) -> Option<bool> {
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
    return Some(start_info.line_number == end_info.line_number
        && start_info.column_number == end_info.column_number);
}

pub fn is_at_same_position(condition_context: &mut ConditionResolverContext, start_info: &Info) -> Option<bool> {
    let start_info = condition_context.get_resolved_info(start_info)?;
    return Some(start_info.line_number == condition_context.writer_info.line_number
        && start_info.column_number == condition_context.writer_info.column_number);
}

pub fn is_on_same_line(condition_context: &mut ConditionResolverContext, start_info: &Info) -> Option<bool> {
    let start_info = condition_context.get_resolved_info(start_info)?;
    return Some(start_info.line_number == condition_context.writer_info.line_number);
}

pub fn is_on_different_line(condition_context: &mut ConditionResolverContext, start_info: &Info) -> Option<bool> {
    let start_info = condition_context.get_resolved_info(start_info)?;
    return Some(start_info.line_number != condition_context.writer_info.line_number);
}

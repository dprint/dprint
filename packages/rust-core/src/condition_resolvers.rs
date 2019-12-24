use super::print_items::*;

pub fn is_start_of_new_line(condition_context: &ConditionResolverContext) -> bool {
    return condition_context.writer_info.column_number == condition_context.writer_info.line_start_column_number;
}

pub fn is_multiple_lines(condition_context: &mut ConditionResolverContext, start_info: &Info, end_info: &Info) -> Option<bool> {
    let resolved_start_info = condition_context.get_resolved_info(&start_info);
    let resolved_end_info = condition_context.get_resolved_info(&end_info);

    if let Some(start_info) = resolved_start_info {
        if let Some(end_info) = resolved_end_info {
            return Some(end_info.line_number > start_info.line_number);
        }
    }

    return Option::None;
}
use super::print_items::*;

pub fn true_resolver() -> ConditionResolver<'static> {
  &|_| Some(true)
}

pub fn false_resolver() -> ConditionResolver<'static> {
  &|_| Some(false)
}

pub fn is_start_of_line() -> ConditionResolver<'static> {
  &|context| Some(context.writer_info.is_start_of_line())
}

pub fn is_not_start_of_line() -> ConditionResolver<'static> {
  &|context| Some(!context.writer_info.is_start_of_line())
}

pub fn is_start_of_line_indented() -> ConditionResolver<'static> {
  &|context| Some(context.writer_info.is_start_of_line_indented())
}

pub fn is_start_of_line_or_is_start_of_line_indented() -> ConditionResolver<'static> {
  &|context| Some(context.writer_info.is_start_of_line_indented() || context.writer_info.is_start_of_line())
}

pub fn is_forcing_no_newlines() -> ConditionResolver<'static> {
  &|context| Some(context.is_forcing_no_newlines())
}

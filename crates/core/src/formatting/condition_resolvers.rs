use std::rc::Rc;

use super::print_items::*;

thread_local! {
  static TRUE: ConditionResolver = Rc::new(|_| {
    Some(true)
  });

  static FALSE: ConditionResolver = Rc::new(|_| {
    Some(false)
  });

  static IS_START_OF_LINE_RESOLVER: ConditionResolver = Rc::new(|context| {
    Some(context.writer_info.is_start_of_line())
  });

  static IS_NOT_START_OF_LINE_RESOLVER: ConditionResolver = Rc::new(|context| {
    Some(!context.writer_info.is_start_of_line())
  });

  static IS_START_OF_LINE_INDENTED: ConditionResolver = Rc::new(|context| {
    Some(context.writer_info.is_start_of_line_indented())
  });

  static IS_START_OF_LINE_OR_IS_START_OF_LINE_INDENTED: ConditionResolver = Rc::new(|context| {
    Some(context.writer_info.is_start_of_line_indented() || context.writer_info.is_start_of_line())
  });

  static IS_FORCING_NO_NEWLINES: ConditionResolver = Rc::new(|context| {
    Some(context.is_forcing_no_newlines())
  });
}

pub fn true_resolver() -> ConditionResolver {
  TRUE.with(|r| r.clone())
}

pub fn false_resolver() -> ConditionResolver {
  FALSE.with(|r| r.clone())
}

pub fn is_start_of_line() -> ConditionResolver {
  IS_START_OF_LINE_RESOLVER.with(|r| r.clone())
}

pub fn is_not_start_of_line() -> ConditionResolver {
  IS_NOT_START_OF_LINE_RESOLVER.with(|r| r.clone())
}

pub fn is_start_of_line_indented() -> ConditionResolver {
  IS_START_OF_LINE_INDENTED.with(|r| r.clone())
}

pub fn is_start_of_line_or_is_start_of_line_indented() -> ConditionResolver {
  IS_START_OF_LINE_OR_IS_START_OF_LINE_INDENTED.with(|r| r.clone())
}

pub fn is_forcing_no_newlines() -> ConditionResolver {
  IS_FORCING_NO_NEWLINES.with(|r| r.clone())
}

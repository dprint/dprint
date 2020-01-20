use super::print_items::*;
use super::*;

pub fn indent_if_start_of_line(elements: Vec<PrintItem>) -> Condition {
    Condition::new("indentIfStartOfLine", ConditionProperties {
        condition: Box::new(|context| Some(condition_resolvers::is_start_of_new_line(&context))),
        true_path: Some(parser_helpers::with_indent(elements.clone())),
        false_path: Some(elements),
    })
}

pub fn with_indent_if_start_of_line_indented(elements: Vec<PrintItem>) -> Condition {
    Condition::new("withIndentIfStartOfLineIndented", ConditionProperties {
        condition: Box::new(|context| Some(context.writer_info.line_start_indent_level > context.writer_info.indent_level)),
        true_path: parser_helpers::with_indent(elements.clone()).into(),
        false_path: elements.into(),
    })
}

pub struct NewLineIfHangingSpaceOtherwiseOptions {
    pub start_info: Info,
    pub end_info: Option<Info>,
    pub space_char: Option<PrintItem>,
}

pub fn new_line_if_hanging_space_otherwise(opts: NewLineIfHangingSpaceOtherwiseOptions) -> Condition {
    let space_char = opts.space_char.unwrap_or(" ".into());
    let start_info = opts.start_info;
    let end_info = opts.end_info;

    Condition::new("newLineIfHangingSpaceOtherwise", ConditionProperties {
        condition: Box::new(move |context| {
            return condition_resolvers::is_hanging(context, &start_info, &end_info);
        }),
        true_path: Some(vec![PrintItem::NewLine]),
        false_path: Some(vec![space_char]),
    })
}

/// This condition can be used to force the printer to jump back to the point
/// this condition exists at once the provided info is resolved.
pub fn force_reevaluation_once_resolved(info: Info) -> Condition {
    Condition::new("forceReevaluationOnceInfoResolved", ConditionProperties {
        condition: Box::new(move |context| {
            let resolved_info = context.get_resolved_info(&info);
            if resolved_info.is_some() { Some(false) } else { None }
        }),
        true_path: None,
        false_path: None,
    })
}

pub fn new_line_if_multiple_lines_space_or_new_line_otherwise(start_info: Info, end_info: Option<Info>) -> Condition {
    Condition::new("newLineIfMultipleLinesSpaceOrNewLineOtherwise", ConditionProperties {
        condition: Box::new(move |context| {
            let start_info = context.get_resolved_info(&start_info)?;
            let end_info = {
                if let Some(end_info) = &end_info {
                    context.get_resolved_info(end_info)?
                } else {
                    context.writer_info.clone()
                }
            };

            return Some(end_info.line_number > start_info.line_number);
        }),
        true_path: Some(vec![PrintItem::NewLine]),
        false_path: Some(vec![PrintItem::SpaceOrNewLine]),
    })
}

pub fn single_indent_if_start_of_line() -> Condition {
    Condition::new("singleIndentIfStartOfLine", ConditionProperties {
        condition: Box::new(|context| Some(condition_resolvers::is_start_of_new_line(context))),
        true_path: Some(vec![PrintItem::SingleIndent]),
        false_path: None
    })
}

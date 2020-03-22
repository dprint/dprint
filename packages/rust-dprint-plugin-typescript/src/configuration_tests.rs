use std::collections::HashMap;
use dprint_core::configuration::{resolve_global_config, NewLineKind};
use super::configuration::*;

#[test]
fn check_all_values_set() {
    let mut config = ConfigurationBuilder::new();
    config.new_line_kind(NewLineKind::Auto)
        .line_width(80)
        .use_tabs(false)
        .indent_width(4)
        /* common */
        .quote_style(QuoteStyle::AlwaysDouble)
        .semi_colons(SemiColons::Prefer)
        .brace_position(BracePosition::NextLine)
        .next_control_flow_position(NextControlFlowPosition::SameLine)
        .operator_position(OperatorPosition::SameLine)
        .single_body_position(SingleBodyPosition::SameLine)
        .trailing_commas(TrailingCommas::Never)
        .use_braces(UseBraces::WhenNotSingleLine)
        /* prefer hanging */
        .prefer_hanging(false)
        .prefer_hanging_arguments(false)
        .prefer_hanging_parameters(false)
        /* use parentheses */
        .arrow_function_use_parentheses(UseParentheses::Maintain)
        /* brace position*/
        .arrow_function_brace_position(BracePosition::NextLine)
        .class_declaration_brace_position(BracePosition::NextLine)
        .class_expression_brace_position(BracePosition::NextLine)
        .constructor_brace_position(BracePosition::NextLine)
        .do_while_statement_brace_position(BracePosition::NextLine)
        .enum_declaration_brace_position(BracePosition::NextLine)
        .for_statement_brace_position(BracePosition::NextLine)
        .for_in_statement_brace_position(BracePosition::NextLine)
        .for_of_statement_brace_position(BracePosition::NextLine)
        .get_accessor_brace_position(BracePosition::NextLine)
        .if_statement_brace_position(BracePosition::NextLine)
        .interface_declaration_brace_position(BracePosition::NextLine)
        .function_declaration_brace_position(BracePosition::NextLine)
        .function_expression_brace_position(BracePosition::NextLine)
        .method_brace_position(BracePosition::NextLine)
        .module_declaration_brace_position(BracePosition::NextLine)
        .set_accessor_brace_position(BracePosition::NextLine)
        .switch_case_brace_position(BracePosition::NextLine)
        .switch_statement_brace_position(BracePosition::NextLine)
        .try_statement_brace_position(BracePosition::NextLine)
        .while_statement_brace_position(BracePosition::NextLine)
        /* prefer hanging */
        .array_expression_prefer_hanging(true)
        .array_pattern_prefer_hanging(true)
        .do_while_statement_prefer_hanging(true)
        .export_declaration_prefer_hanging(true)
        .extends_clause_prefer_hanging(true)
        .for_in_statement_prefer_hanging(true)
        .for_of_statement_prefer_hanging(true)
        .for_statement_prefer_hanging(true)
        .if_statement_prefer_hanging(true)
        .implements_clause_prefer_hanging(true)
        .import_declaration_prefer_hanging(true)
        .object_expression_prefer_hanging(true)
        .sequence_expression_prefer_hanging(true)
        .switch_statement_prefer_hanging(true)
        .tuple_type_prefer_hanging(true)
        .type_literal_prefer_hanging(true)
        .type_parameter_declaration_prefer_hanging(true)
        .union_and_intersection_type_prefer_hanging(true)
        .while_statement_prefer_hanging(true)
        /* prefer hanging arguments */
        .call_expression_prefer_hanging_arguments(true)
        .new_expression_prefer_hanging_arguments(true)
        /* prefer hanging parameters */
        .arrow_function_prefer_hanging_parameters(true)
        .call_signature_prefer_hanging_parameters(true)
        .construct_signature_prefer_hanging_parameters(true)
        .constructor_prefer_hanging_parameters(true)
        .constructor_type_prefer_hanging_parameters(true)
        .function_declaration_prefer_hanging_parameters(true)
        .function_expression_prefer_hanging_parameters(true)
        .function_type_prefer_hanging_parameters(true)
        .get_accessor_prefer_hanging_parameters(true)
        .method_prefer_hanging_parameters(true)
        .method_signature_prefer_hanging_parameters(true)
        .set_accessor_prefer_hanging_parameters(true)
        /* member spacing */
        .enum_declaration_member_spacing(MemberSpacing::Maintain)
        /* next control flow position */
        .if_statement_next_control_flow_position(NextControlFlowPosition::SameLine)
        .try_statement_next_control_flow_position(NextControlFlowPosition::SameLine)
        /* operator position */
        .binary_expression_operator_position(OperatorPosition::SameLine)
        .conditional_expression_operator_position(OperatorPosition::SameLine)
        /* single body position */
        .if_statement_single_body_position(SingleBodyPosition::SameLine)
        .for_statement_single_body_position(SingleBodyPosition::SameLine)
        .for_in_statement_single_body_position(SingleBodyPosition::SameLine)
        .for_of_statement_single_body_position(SingleBodyPosition::SameLine)
        .while_statement_single_body_position(SingleBodyPosition::SameLine)
        /* trailing commas */
        .arguments_trailing_commas(TrailingCommas::Never)
        .parameters_trailing_commas(TrailingCommas::Never)
        .array_expression_trailing_commas(TrailingCommas::Never)
        .array_pattern_trailing_commas(TrailingCommas::Never)
        .enum_declaration_trailing_commas(TrailingCommas::Never)
        .object_expression_trailing_commas(TrailingCommas::Never)
        .type_parameter_declaration_trailing_commas(TrailingCommas::Never)
        .tuple_type_trailing_commas(TrailingCommas::Never)
        /* use braces */
        .if_statement_use_braces(UseBraces::Always)
        .for_statement_use_braces(UseBraces::Always)
        .for_in_statement_use_braces(UseBraces::Always)
        .for_of_statement_use_braces(UseBraces::Always)
        .while_statement_use_braces(UseBraces::Always)
        /* space settings */
        .binary_expression_space_surrounding_bitwise_and_arithmetic_operator(true)
        .comment_line_force_space_after_slashes(false)
        .construct_signature_space_after_new_keyword(true)
        .constructor_space_before_parentheses(true)
        .constructor_type_space_after_new_keyword(true)
        .do_while_statement_space_after_while_keyword(true)
        .export_declaration_space_surrounding_named_exports(true)
        .for_statement_space_after_for_keyword(true)
        .for_statement_space_after_semi_colons(true)
        .for_in_statement_space_after_for_keyword(true)
        .for_of_statement_space_after_for_keyword(true)
        .function_declaration_space_before_parentheses(true)
        .function_expression_space_before_parentheses(true)
        .function_expression_space_after_function_keyword(true)
        .get_accessor_space_before_parentheses(true)
        .if_statement_space_after_if_keyword(true)
        .import_declaration_space_surrounding_named_imports(true)
        .jsx_expression_container_space_surrounding_expression(true)
        .method_space_before_parentheses(true)
        .set_accessor_space_before_parentheses(true)
        .tagged_template_space_before_literal(false)
        .type_annotation_space_before_colon(true)
        .type_assertion_space_before_expression(true)
        .while_statement_space_after_while_keyword(true);

    let inner_config = config.get_inner_config();
    assert_eq!(inner_config.len(), 117);
    let diagnostics = resolve_config(&inner_config, &resolve_global_config(&HashMap::new()).config).diagnostics;
    assert_eq!(diagnostics.len(), 0);
}

#[test]
fn handle_global_config() {
    let mut global_config = HashMap::new();
    global_config.insert(String::from("lineWidth"), String::from("80"));
    global_config.insert(String::from("indentWidth"), String::from("8"));
    global_config.insert(String::from("newLineKind"), String::from("crlf"));
    global_config.insert(String::from("useTabs"), String::from("true"));
    let global_config = resolve_global_config(&global_config).config;
    let mut config_builder = ConfigurationBuilder::new();
    let config = config_builder.global_config(global_config).build();
    assert_eq!(config.line_width, 80);
    assert_eq!(config.indent_width, 8);
    assert_eq!(config.new_line_kind == NewLineKind::CarriageReturnLineFeed, true);
    assert_eq!(config.use_tabs, true);
}

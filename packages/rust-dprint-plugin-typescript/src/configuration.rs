use std::collections::HashMap;

#[derive(Clone)]
pub struct TypeScriptConfiguration {
    pub single_quotes: bool,
    pub line_width: u32,
    /* semi-colon */
    pub break_statement_semi_colon: bool,
    pub continue_statement_semi_colon: bool,
    pub debugger_statement_semi_colon: bool,
    pub empty_statement_semi_colon: bool,
    pub export_assignment_semi_colon: bool,
    pub expression_statement_semi_colon: bool,
    /* force multi-line arguments */
    pub call_expression_force_multi_line_arguments: bool,
    /* trailing commas */
    pub array_expression_trialing_commas: TrailingCommas,
    pub array_pattern_trialing_commas: TrailingCommas,
    /* use space separator */
    pub type_annotation_space_before_colon: bool,
}

/// Trailing comma possibilities.
#[derive(Clone)]
pub enum TrailingCommas {
    /// Trailing commas should not be used.
    Never,
    /// Trailing commas should always be used.
    Always,
    /// Trailing commas should only be used in multi-line scenarios.
    OnlyMultiLine
}

pub fn resolve_config(config: &HashMap<String, String>) -> TypeScriptConfiguration {
    let mut config = config.clone();
    let semi_colons = get_value(&mut config, "semiColons", true);
    let force_multi_line_arguments = get_value(&mut config, "forceMultiLineArguments", false);
    let trailing_commas = get_trailing_commas(&mut config, "trailingCommas", &TrailingCommas::Never);

    let resolved_config = TypeScriptConfiguration {
        line_width: get_value(&mut config, "lineWidth", 120),
        single_quotes: get_value(&mut config, "singleQuotes", false),
        /* semi-colon */
        break_statement_semi_colon: get_value(&mut config, "breakStatement.semiColon", semi_colons),
        continue_statement_semi_colon: get_value(&mut config, "continueStatement.semiColon", semi_colons),
        debugger_statement_semi_colon: get_value(&mut config, "debuggerStatement.semiColon", semi_colons),
        empty_statement_semi_colon: get_value(&mut config, "emptyStatement.semiColon", semi_colons),
        export_assignment_semi_colon: get_value(&mut config, "exportAssignment.semiColon", semi_colons),
        expression_statement_semi_colon: get_value(&mut config, "expressionStatement.semiColon", semi_colons),
        /* force multi-line arguments */
        call_expression_force_multi_line_arguments: get_value(&mut config, "callExpression.forceMultiLineArguments", force_multi_line_arguments),
        /* trailing commas */
        array_expression_trialing_commas: get_trailing_commas(&mut config, "arrayExpression.trailingCommas", &trailing_commas),
        array_pattern_trialing_commas: get_trailing_commas(&mut config, "arrayPattern.trailingCommas", &trailing_commas),
        /* space separator */
        type_annotation_space_before_colon: get_value(&mut config, "typeAnnotation.spaceBeforeColon", false),
    };

    if !config.is_empty() {
        panic!("Unhandled configuration value(s): {}", config.keys().map(|x| x.to_owned()).collect::<Vec<String>>().join(", "));
    }

    return resolved_config;
}

fn get_value<T>(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: T
) -> T where T : std::str::FromStr, <T as std::str::FromStr>::Err : std::fmt::Debug {
    let value = config.get(prop).map(|x| x.parse::<T>().unwrap()).unwrap_or(default_value);
    config.remove(prop);
    return value;
}

fn get_trailing_commas(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: &TrailingCommas
) -> TrailingCommas {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "always" => TrailingCommas::Always,
            "never" => TrailingCommas::Never,
            "onlyMultiLine" => TrailingCommas::OnlyMultiLine,
            "" => default_value.clone(),
            _ => panic!("Invalid configuration option.") // todo: diagnostics instead
        }
    } else {
        default_value.clone()
    }
}
use std::collections::HashMap;
use super::*;

pub fn resolve_config(config: &HashMap<String, String>) -> TypeScriptConfiguration {
    let mut config = config.clone();
    let semi_colons = get_value(&mut config, "semiColons", true);
    let force_multi_line_arguments = get_value(&mut config, "forceMultiLineArguments", false);

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
use std::collections::HashMap;
use super::*;

pub fn resolve_config(config: &HashMap<String, String>) -> TypeScriptConfiguration {
    let mut config = config.clone();
    let semi_colons = get_value(&mut config, "semiColons", true);

    let resolved_config = TypeScriptConfiguration {
        line_width: get_value(&mut config, "lineWidth", 120),
        single_quotes: get_value(&mut config, "singleQuotes", false),
        /* semi-colon */
        expression_statement_semi_colon: get_value(&mut config, "expressionStatement.semiColon", semi_colons),
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
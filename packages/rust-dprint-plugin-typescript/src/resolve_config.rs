use std::collections::HashMap;
use super::*;

pub fn resolve_config(config: &HashMap<String, String>) -> TypeScriptConfiguration {
    let mut config = config.clone();
    let semi_colons = get_bool(&mut config, "semiColons", true);

    let resolved_config = TypeScriptConfiguration {
        single_quotes: get_bool(&mut config, "singleQuotes", false),
        /* semi-colon */
        expression_statement_semi_colon: get_bool(&mut config, "expressionStatement.semiColon", semi_colons),
    };

    if !config.is_empty() {
        panic!("Unhandled configuration value(s): {}", config.keys().map(|x| x.to_owned()).collect::<Vec<String>>().join(", "));
    }

    return resolved_config;
}

fn get_bool(config: &mut HashMap<String, String>, prop: &str, default_value: bool) -> bool {
    let value = config.get(prop).map(|x| x.parse::<bool>().unwrap()).unwrap_or(default_value);
    config.remove(prop);
    return value;
}
pub fn is_prefix_semi_colon_insertion_char(value: char) -> bool {
    match value {
        // from: https://standardjs.com/rules.html#semicolons
        '[' | '(' | '`' | '+' | '*' | '/' | '-' | ',' | '.' => true,
        _ => false,
    }
}

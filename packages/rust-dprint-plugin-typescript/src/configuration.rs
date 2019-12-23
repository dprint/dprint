#[derive(Clone)]
pub struct TypeScriptConfiguration {
    pub single_quotes: bool,
    pub line_width: u32,
    /* semi-colon */
    pub expression_statement_semi_colon: bool,
}

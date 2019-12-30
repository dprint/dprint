use std::collections::HashMap;

#[derive(Clone)]
pub struct TypeScriptConfiguration {
    pub single_quotes: bool,
    pub line_width: u32,
    pub use_tabs: bool,
    pub indent_width: u8,
    /* use parentheses */
    pub arrow_function_expression_use_parentheses: UseParentheses,
    /* brace position */
    pub arrow_function_expression_brace_position: BracePosition,
    pub enum_declaration_brace_position: BracePosition,
    pub interface_declaration_brace_position: BracePosition,
    pub function_declaration_brace_position: BracePosition,
    pub function_expression_brace_position: BracePosition,
    /* force multi-line arguments */
    pub call_expression_force_multi_line_arguments: bool,
    pub new_expression_force_multi_line_arguments: bool,
    /* force multi-line parameters */
    pub arrow_function_expression_force_multi_line_parameters: bool,
    pub call_signature_force_multi_line_parameters: bool,
    pub construct_signature_force_multi_line_parameters: bool,
    pub function_declaration_force_multi_line_parameters: bool,
    pub function_expression_force_multi_line_parameters: bool,
    pub method_signature_force_multi_line_parameters: bool,
    /* member spacing */
    pub enum_declaration_member_spacing: MemberSpacing,
    /* operator position */
    pub binary_expression_operator_position: OperatorPosition,
    pub conditional_expression_operator_position: OperatorPosition,
    /* semi-colon */
    pub break_statement_semi_colon: bool,
    pub call_signature_semi_colon: bool,
    pub construct_signature_semi_colon: bool,
    pub continue_statement_semi_colon: bool,
    pub debugger_statement_semi_colon: bool,
    pub empty_statement_semi_colon: bool,
    pub export_all_declaration_semi_colon: bool,
    pub export_assignment_semi_colon: bool,
    pub export_default_expression_semi_colon: bool,
    pub export_named_declaration_semi_colon: bool,
    pub expression_statement_semi_colon: bool,
    pub function_declaration_semi_colon: bool,
    pub import_declaration_semi_colon: bool,
    pub import_equals_semi_colon: bool,
    pub index_signature_semi_colon: bool,
    pub method_signature_semi_colon: bool,
    pub namespace_export_declaration_semi_colon: bool,
    pub property_signature_semi_colon: bool,
    pub return_statement_semi_colon: bool,
    pub throw_statement_semi_colon: bool,
    pub type_alias_semi_colon: bool,
    pub variable_statement_semi_colon: bool,
    /* trailing commas */
    pub array_expression_trailing_commas: TrailingCommas,
    pub array_pattern_trailing_commas: TrailingCommas,
    pub enum_declaration_trailing_commas: TrailingCommas,
    pub object_expression_trailing_commas: TrailingCommas,

    /* use space separator */

    /// Whether to add a space after the `new` keyword in a construct signature.
    /// `true` - Ex. `new (): MyClass;`
    /// `false` (default) - Ex. `new(): MyClass;`
    pub construct_signature_space_after_new_keyword: bool,
    /// Whether to add spaces around named exports in an export declaration.
    /// * `true` (default) - Ex. `export { SomeExport, OtherExport };`
    /// * `false` - Ex. `export {SomeExport, OtherExport};`
    pub export_declaration_space_surrounding_named_exports: bool,
    /// Whether to add a space before the parentheses of a function declaration.
    /// * `true` - Ex. `function myFunction ()`
    /// * `false` (default) - Ex. `function myFunction()`
    pub function_declaration_space_before_parentheses: bool,
    /// Whether to add a space before the parentheses of a function expression.
    /// `true` - Ex. `function ()`
    /// `false` (default) - Ex. `function()`
    pub function_expression_space_before_parentheses: bool,
    /// Whether to add spaces around named imports in an import declaration.
    /// * `true` (default) - Ex. `import { SomeExport, OtherExport } from "my-module";`
    /// * `false` - Ex. `import {SomeExport, OtherExport} from "my-module";`
    pub import_declaration_space_surrounding_named_imports: bool,
    /// Whether to surround bitwise and arithmetic operators in a binary expression with spaces.
    /// * `true` (default) - Ex. `1 + 2`
    /// * `false` - Ex. `1+2`
    pub binary_expression_space_surrounding_bitwise_and_arithmetic_operator: bool,
    /// Whether to add a space before the colon of a type annotation.
    /// * `true` - Ex. `function myFunction() : string`
    /// * `false` (default) - Ex. `function myFunction(): string`
    pub type_annotation_space_before_colon: bool,
    /// Whether to add a space before the expression in a type assertion.
    /// * `true` (default) - Ex. `<string> myValue`
    /// * `false` - Ex. `<string>myValue`
    pub type_assertion_space_before_expression: bool,
}

/// Trailing comma possibilities.
#[derive(Clone, PartialEq)]
pub enum TrailingCommas {
    /// Trailing commas should not be used.
    Never,
    /// Trailing commas should always be used.
    Always,
    /// Trailing commas should only be used in multi-line scenarios.
    OnlyMultiLine,
}

/// Where to place the opening brace.
#[derive(Clone, PartialEq)]
pub enum BracePosition {
    /// Maintains the brace being on the next line or the same line.
    Maintain,
    /// Forces the brace to be on the same line.
    SameLine,
    /// Forces the brace to be on the next line.
    NextLine,
    /// Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
    NextLineIfHanging,
}

/// How to space members.
#[derive(Clone, PartialEq)]
pub enum MemberSpacing {
    /// Maintains whether a newline or blankline is used.
    Maintain,
    /// Forces a new line between members.
    NewLine,
    /// Forces a blank line between members.
    BlankLine,
}

/// Whether to use parentheses around a single parameter in an arrow function.
#[derive(Clone, PartialEq)]
pub enum UseParentheses {
    /// Maintains the current state of the parentheses.
    Maintain,
    /// Forces parentheses.
    Force,
    /// Prefers not using parentheses when possible.
    PreferNone,
}

/// Where to place the operator for expressions that span multiple lines.
#[derive(Clone, PartialEq)]
pub enum OperatorPosition {
    /// Maintains the operator being on the next line or the same line.
    Maintain,
    /// Forces the operator to be on the same line.
    SameLine,
    /// Forces the operator to be on the next line.
    NextLine,
}

pub fn resolve_config(config: &HashMap<String, String>) -> TypeScriptConfiguration {
    let mut config = config.clone();
    let semi_colons = get_value(&mut config, "semiColons", true);
    let force_multi_line_arguments = get_value(&mut config, "forceMultiLineArguments", false);
    let force_multi_line_parameters = get_value(&mut config, "forceMultiLineParameters", false);
    let trailing_commas = get_trailing_commas(&mut config, "trailingCommas", &TrailingCommas::Never);
    let brace_position = get_brace_position(&mut config, "bracePosition", &BracePosition::NextLineIfHanging);
    let operator_position = get_operator_position(&mut config, "operatorPosition", &OperatorPosition::NextLine);

    let resolved_config = TypeScriptConfiguration {
        line_width: get_value(&mut config, "lineWidth", 120),
        use_tabs: get_value(&mut config, "useTabs", false),
        indent_width: get_value(&mut config, "indentWidth", 4),
        single_quotes: get_value(&mut config, "singleQuotes", false),
        /* use parentheses */
        arrow_function_expression_use_parentheses: get_use_parentheses(&mut config, "arrowFunctionExpression.useParentheses", &UseParentheses::Maintain),
        /* brace position */
        arrow_function_expression_brace_position: get_brace_position(&mut config, "arrowFunctionExpression.bracePosition", &brace_position),
        enum_declaration_brace_position: get_brace_position(&mut config, "enumDeclaration.bracePosition", &brace_position),
        interface_declaration_brace_position: get_brace_position(&mut config, "interfaceDeclaration.bracePosition", &brace_position),
        function_declaration_brace_position: get_brace_position(&mut config, "functionDeclaration.bracePosition", &brace_position),
        function_expression_brace_position: get_brace_position(&mut config, "functionExpression.bracePosition", &brace_position),
        /* force multi-line arguments */
        call_expression_force_multi_line_arguments: get_value(&mut config, "callExpression.forceMultiLineArguments", force_multi_line_arguments),
        new_expression_force_multi_line_arguments: get_value(&mut config, "newExpression.forceMultiLineArguments", force_multi_line_arguments),
        /* force multi-line parameters */
        arrow_function_expression_force_multi_line_parameters: get_value(&mut config, "arrowFunctionExpression.forceMultiLineParameters", force_multi_line_parameters),
        call_signature_force_multi_line_parameters: get_value(&mut config, "callSignature.forceMultiLineParameters", force_multi_line_parameters),
        construct_signature_force_multi_line_parameters: get_value(&mut config, "constructSignature.forceMultiLineParameters", force_multi_line_parameters),
        function_declaration_force_multi_line_parameters: get_value(&mut config, "functionDeclaration.forceMultiLineParameters", force_multi_line_parameters),
        function_expression_force_multi_line_parameters: get_value(&mut config, "functionExpression.forceMultiLineParameters", force_multi_line_parameters),
        method_signature_force_multi_line_parameters: get_value(&mut config, "methodSignature.forceMultiLineParameters", force_multi_line_parameters),
        /* member spacing */
        enum_declaration_member_spacing: get_member_spacing(&mut config, "enumDeclaration.memberSpacing", &MemberSpacing::Maintain),
        /* operator position */
        binary_expression_operator_position: get_operator_position(&mut config, "binaryExpression.operatorPosition", &operator_position),
        conditional_expression_operator_position: get_operator_position(&mut config, "conditionalExpression.operatorPosition", &operator_position),
        /* semi-colon */
        break_statement_semi_colon: get_value(&mut config, "breakStatement.semiColon", semi_colons),
        call_signature_semi_colon: get_value(&mut config, "callSignature.semiColon", semi_colons),
        construct_signature_semi_colon: get_value(&mut config, "constructSignature.semiColon", semi_colons),
        continue_statement_semi_colon: get_value(&mut config, "continueStatement.semiColon", semi_colons),
        debugger_statement_semi_colon: get_value(&mut config, "debuggerStatement.semiColon", semi_colons),
        empty_statement_semi_colon: get_value(&mut config, "emptyStatement.semiColon", semi_colons),
        export_all_declaration_semi_colon: get_value(&mut config, "exportAllDeclaration.semiColon", semi_colons),
        export_assignment_semi_colon: get_value(&mut config, "exportAssignment.semiColon", semi_colons),
        export_default_expression_semi_colon: get_value(&mut config, "exportDefaultExpression.semiColon", semi_colons),
        export_named_declaration_semi_colon: get_value(&mut config, "exportNamedDeclaration.semiColon", semi_colons),
        expression_statement_semi_colon: get_value(&mut config, "expressionStatement.semiColon", semi_colons),
        function_declaration_semi_colon: get_value(&mut config, "functionDeclaration.semiColon", semi_colons),
        import_declaration_semi_colon: get_value(&mut config, "importDeclaration.semiColon", semi_colons),
        import_equals_semi_colon: get_value(&mut config, "importEqualsDeclaration.semiColon", semi_colons),
        index_signature_semi_colon: get_value(&mut config, "indexSignature.semiColon", semi_colons),
        method_signature_semi_colon: get_value(&mut config, "methodSignature.semiColon", semi_colons),
        namespace_export_declaration_semi_colon: get_value(&mut config, "namespaceExportDeclaration.semiColon", semi_colons),
        property_signature_semi_colon: get_value(&mut config, "propertySignature.semiColon", semi_colons),
        return_statement_semi_colon: get_value(&mut config, "returnStatement.semiColon", semi_colons),
        throw_statement_semi_colon: get_value(&mut config, "throwStatement.semiColon", semi_colons),
        type_alias_semi_colon: get_value(&mut config, "typeAlias.semiColon", semi_colons),
        variable_statement_semi_colon: get_value(&mut config, "variableStatement.semiColon", semi_colons),
        /* trailing commas */
        array_expression_trailing_commas: get_trailing_commas(&mut config, "arrayExpression.trailingCommas", &trailing_commas),
        array_pattern_trailing_commas: get_trailing_commas(&mut config, "arrayPattern.trailingCommas", &trailing_commas),
        enum_declaration_trailing_commas: get_trailing_commas(&mut config, "enumDeclaration.trailingCommas", &trailing_commas),
        object_expression_trailing_commas: get_trailing_commas(&mut config, "objectExpression.trailingCommas", &trailing_commas),
        /* space settings */
        construct_signature_space_after_new_keyword: get_value(&mut config, "constructSignature.spaceAfterNewKeyword", false),
        export_declaration_space_surrounding_named_exports: get_value(&mut config, "exportDeclaration.spaceSurroundingNamedExports", true),
        function_declaration_space_before_parentheses: get_value(&mut config, "functionDeclaration.spaceBeforeParentheses", false),
        function_expression_space_before_parentheses: get_value(&mut config, "functionExpression.spaceBeforeParentheses", false),
        import_declaration_space_surrounding_named_imports: get_value(&mut config, "importDeclaration.spaceSurroundingNamedImports", true),
        binary_expression_space_surrounding_bitwise_and_arithmetic_operator: get_value(&mut config, "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator", true),
        type_annotation_space_before_colon: get_value(&mut config, "typeAnnotation.spaceBeforeColon", false),
        type_assertion_space_before_expression: get_value(&mut config, "typeAssertion.spaceBeforeExpression", true),
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
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value.clone()
    }
}

fn get_brace_position(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: &BracePosition
) -> BracePosition {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => BracePosition::Maintain,
            "sameLine" => BracePosition::SameLine,
            "nextLine" => BracePosition::NextLine,
            "nextLineIfHanging" => BracePosition::NextLineIfHanging,
            "" => default_value.clone(),
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value.clone()
    }
}

fn get_member_spacing(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: &MemberSpacing
) -> MemberSpacing {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => MemberSpacing::Maintain,
            "blankline" => MemberSpacing::BlankLine,
            "newline" => MemberSpacing::NewLine,
            "" => default_value.clone(),
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value.clone()
    }
}

fn get_operator_position(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: &OperatorPosition
) -> OperatorPosition {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => OperatorPosition::Maintain,
            "sameLine" => OperatorPosition::SameLine,
            "nextLine" => OperatorPosition::NextLine,
            "" => default_value.clone(),
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value.clone()
    }
}

fn get_use_parentheses(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: &UseParentheses
) -> UseParentheses {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => UseParentheses::Maintain,
            "force" => UseParentheses::Force,
            "preferNone" => UseParentheses::PreferNone,
            "" => default_value.clone(),
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value.clone()
    }
}
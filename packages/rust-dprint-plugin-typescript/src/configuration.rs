use std::collections::HashMap;

#[derive(Clone)]
pub struct TypeScriptConfiguration {
    pub new_line_kind: NewLineKind,
    pub single_quotes: bool,
    pub line_width: u32,
    pub use_tabs: bool,
    pub indent_width: u8,
    /* use parentheses */
    pub arrow_function_expression_use_parentheses: UseParentheses,
    /* brace position */
    pub arrow_function_expression_brace_position: BracePosition,
    pub class_declaration_brace_position: BracePosition,
    pub class_expression_brace_position: BracePosition,
    pub constructor_brace_position: BracePosition,
    pub do_while_statement_brace_position: BracePosition,
    pub enum_declaration_brace_position: BracePosition,
    pub get_accessor_brace_position: BracePosition,
    pub if_statement_brace_position: BracePosition,
    pub interface_declaration_brace_position: BracePosition,
    pub for_statement_brace_position: BracePosition,
    pub for_in_statement_brace_position: BracePosition,
    pub for_of_statement_brace_position: BracePosition,
    pub function_declaration_brace_position: BracePosition,
    pub function_expression_brace_position: BracePosition,
    pub method_brace_position: BracePosition,
    pub module_declaration_brace_position: BracePosition,
    pub set_accessor_brace_position: BracePosition,
    pub switch_case_brace_position: BracePosition,
    pub switch_statement_brace_position: BracePosition,
    pub try_statement_brace_position: BracePosition,
    pub while_statement_brace_position: BracePosition,
    /* force multi-line arguments */
    pub call_expression_force_multi_line_arguments: bool,
    pub new_expression_force_multi_line_arguments: bool,
    /* force multi-line parameters */
    pub arrow_function_expression_force_multi_line_parameters: bool,
    pub call_signature_force_multi_line_parameters: bool,
    pub construct_signature_force_multi_line_parameters: bool,
    pub constructor_force_multi_line_parameters: bool,
    pub constructor_type_force_multi_line_parameters: bool,
    pub function_declaration_force_multi_line_parameters: bool,
    pub function_expression_force_multi_line_parameters: bool,
    pub function_type_force_multi_line_parameters: bool,
    pub get_accessor_force_multi_line_parameters: bool,
    pub method_force_multi_line_parameters: bool,
    pub method_signature_force_multi_line_parameters: bool,
    pub set_accessor_force_multi_line_parameters: bool,
    /* member spacing */
    pub enum_declaration_member_spacing: MemberSpacing,
    /* next control flow position */
    pub if_statement_next_control_flow_position: NextControlFlowPosition,
    pub try_statement_next_control_flow_position: NextControlFlowPosition,
    /* operator position */
    pub binary_expression_operator_position: OperatorPosition,
    pub conditional_expression_operator_position: OperatorPosition,
    /* semi-colon */
    pub break_statement_semi_colon: bool,
    pub call_signature_semi_colon: bool,
    pub class_property_semi_colon: bool,
    pub construct_signature_semi_colon: bool,
    pub constructor_semi_colon: bool,
    pub continue_statement_semi_colon: bool,
    pub debugger_statement_semi_colon: bool,
    pub do_while_statement_semi_colon: bool,
    pub empty_statement_semi_colon: bool,
    pub export_all_declaration_semi_colon: bool,
    pub export_assignment_semi_colon: bool,
    pub export_default_expression_semi_colon: bool,
    pub export_named_declaration_semi_colon: bool,
    pub expression_statement_semi_colon: bool,
    pub function_declaration_semi_colon: bool,
    pub get_accessor_semi_colon: bool,
    pub import_declaration_semi_colon: bool,
    pub import_equals_semi_colon: bool,
    pub index_signature_semi_colon: bool,
    pub mapped_type_semi_colon: bool,
    pub method_semi_colon: bool,
    pub method_signature_semi_colon: bool,
    pub module_declaration_semi_colon: bool,
    pub namespace_export_declaration_semi_colon: bool,
    pub property_signature_semi_colon: bool,
    pub return_statement_semi_colon: bool,
    pub set_accessor_semi_colon: bool,
    pub throw_statement_semi_colon: bool,
    pub type_alias_semi_colon: bool,
    pub variable_statement_semi_colon: bool,
    /* single body position */
    pub if_statement_single_body_position: SingleBodyPosition,
    pub for_statement_single_body_position: SingleBodyPosition,
    pub for_in_statement_single_body_position: SingleBodyPosition,
    pub for_of_statement_single_body_position: SingleBodyPosition,
    pub while_statement_single_body_position: SingleBodyPosition,
    /* trailing commas */
    pub array_expression_trailing_commas: TrailingCommas,
    pub array_pattern_trailing_commas: TrailingCommas,
    pub enum_declaration_trailing_commas: TrailingCommas,
    pub object_expression_trailing_commas: TrailingCommas,
    pub tuple_type_trailing_commas: TrailingCommas,
    /* use braces */
    pub if_statement_use_braces: UseBraces,
    pub for_statement_use_braces: UseBraces,
    pub for_of_statement_use_braces: UseBraces,
    pub for_in_statement_use_braces: UseBraces,
    pub while_statement_use_braces: UseBraces,

    /* use space separator */

    /// Whether to surround bitwise and arithmetic operators in a binary expression with spaces.
    /// * `true` (default) - Ex. `1 + 2`
    /// * `false` - Ex. `1+2`
    pub binary_expression_space_surrounding_bitwise_and_arithmetic_operator: bool,
    /// Whether to add a space after the `new` keyword in a construct signature.
    /// `true` - Ex. `new (): MyClass;`
    /// `false` (default) - Ex. `new(): MyClass;`
    pub construct_signature_space_after_new_keyword: bool,
    /// Whether to add a space before the parentheses of a constructor.
    /// `true` - Ex. `constructor ()`
    /// `false` (false) - Ex. `constructor()`
    pub constructor_space_before_parentheses: bool,
    /// Whether to add a space after the `new` keyword in a constructor type.
    /// `true` - Ex. `type MyClassCtor = new () => MyClass;`
    /// `false` (default) - Ex. `type MyClassCtor = new() => MyClass;`
    pub constructor_type_space_after_new_keyword: bool,
    /// Whether to add a space after the `while` keyword in a do while statement.
    /// `true` (true) - Ex. `do {\n} while (condition);`
    /// `false` - Ex. `do {\n} while(condition);`
    pub do_while_statement_space_after_while_keyword: bool,
    /// Whether to add spaces around named exports in an export declaration.
    /// * `true` (default) - Ex. `export { SomeExport, OtherExport };`
    /// * `false` - Ex. `export {SomeExport, OtherExport};`
    pub export_declaration_space_surrounding_named_exports: bool,
    /// Whether to add a space after the `for` keyword in a "for" statement.
    /// * `true` (default) - Ex. `for (let i = 0; i < 5; i++)`
    /// * `false` - Ex. `for(let i = 0; i < 5; i++)`
    pub for_statement_space_after_for_keyword: bool,
    /// Whether to add a space after the semi-colons in a "for" statement.
    /// * `true` (default) - Ex. `for (let i = 0; i < 5; i++)`
    /// * `false` - Ex. `for (let i = 0;i < 5;i++)`
    pub for_statement_space_after_semi_colons: bool,
    /// Whether to add a space after the `for` keyword in a "for in" statement.
    /// * `true` (default) - Ex. `for (const prop in obj)`
    /// * `false` - Ex. `for(const prop in obj)`
    pub for_in_statement_space_after_for_keyword: bool,
    /// Whether to add a space after the `for` keyword in a "for of" statement.
    /// * `true` (default) - Ex. `for (const value of myArray)`
    /// * `false` - Ex. `for(const value of myArray)`
    pub for_of_statement_space_after_for_keyword: bool,
    /// Whether to add a space before the parentheses of a function declaration.
    /// * `true` - Ex. `function myFunction ()`
    /// * `false` (default) - Ex. `function myFunction()`
    pub function_declaration_space_before_parentheses: bool,
    /// Whether to add a space before the parentheses of a function expression.
    /// `true` - Ex. `function ()`
    /// `false` (default) - Ex. `function()`
    pub function_expression_space_before_parentheses: bool,
    /// Whether to add a space before the parentheses of a get accessor.
    /// `true` - Ex. `get myProp ()`
    /// `false` (false) - Ex. `get myProp()`
    pub get_accessor_space_before_parentheses: bool,
    /// Whether to add a space after the `if` keyword in an "if" statement.
    /// `true` (default) - Ex. `if (true)`
    /// `false` - Ex. `if(true)`
    pub if_statement_space_after_if_keyword: bool,
    /// Whether to add spaces around named imports in an import declaration.
    /// * `true` (default) - Ex. `import { SomeExport, OtherExport } from "my-module";`
    /// * `false` - Ex. `import {SomeExport, OtherExport} from "my-module";`
    pub import_declaration_space_surrounding_named_imports: bool,
    /// Whether to add a space surrounding the expression of a JSX container.
    /// * `true` - Ex. `{ myValue }`
    /// * `false` (default) - Ex. `{myValue}`
    pub jsx_expression_container_space_surrounding_expression: bool,
    /// Whether to add a space before the parentheses of a method.
    /// `true` - Ex. `myMethod ()`
    /// `false` - Ex. `myMethod()`
    pub method_space_before_parentheses: bool,
    /// Whether to add a space before the parentheses of a set accessor.
    /// `true` - Ex. `set myProp (value: string)`
    /// `false` (default) - Ex. `set myProp(value: string)`
    pub set_accessor_space_before_parentheses: bool,
    /// Whether to add a space before the colon of a type annotation.
    /// * `true` - Ex. `function myFunction() : string`
    /// * `false` (default) - Ex. `function myFunction(): string`
    pub type_annotation_space_before_colon: bool,
    /// Whether to add a space before the expression in a type assertion.
    /// * `true` (default) - Ex. `<string> myValue`
    /// * `false` - Ex. `<string>myValue`
    pub type_assertion_space_before_expression: bool,
    /// Whether to add a space after the `while` keyword in a while statement.
    /// * `true` (default) - Ex. `while (true)`
    /// * `false` - Ex. `while(true)`
    pub while_statement_space_after_while_keyword: bool,
}

// todo: maybe move NewLineKind to core? and then maybe re-export it here?
#[derive(Clone, PartialEq, Copy)]
pub enum NewLineKind {
    /// Decide which newline kind to use based on the last newline in the file.
    Auto,
    /// Use slash n new lines.
    Unix,
    /// Use slash r slash n new lines.
    Windows,
}


/// Trailing comma possibilities.
#[derive(Clone, PartialEq, Copy)]
pub enum TrailingCommas {
    /// Trailing commas should not be used.
    Never,
    /// Trailing commas should always be used.
    Always,
    /// Trailing commas should only be used in multi-line scenarios.
    OnlyMultiLine,
}

/// Where to place the opening brace.
#[derive(Clone, PartialEq, Copy)]
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
#[derive(Clone, PartialEq, Copy)]
pub enum MemberSpacing {
    /// Maintains whether a newline or blankline is used.
    Maintain,
    /// Forces a new line between members.
    NewLine,
    /// Forces a blank line between members.
    BlankLine,
}

/// Where to place the next control flow within a control flow statement.
#[derive(Clone, PartialEq, Copy)]
pub enum NextControlFlowPosition {
    /// Maintains the next control flow being on the next line or the same line.
    Maintain,
    /// Forces the next control flow to be on the same line.
    SameLine,
    /// Forces the next control flow to be on the next line.
    NextLine,
}

/// Where to place the operator for expressions that span multiple lines.
#[derive(Clone, PartialEq, Copy)]
pub enum OperatorPosition {
    /// Maintains the operator being on the next line or the same line.
    Maintain,
    /// Forces the operator to be on the same line.
    SameLine,
    /// Forces the operator to be on the next line.
    NextLine,
}

/// Where to place the expression of a statement that could possibly be on one line (ex. `if (true) console.log(5);`).
#[derive(Clone, PartialEq, Copy)]
pub enum SingleBodyPosition {
    /// Maintains the position of the expression.
    Maintain,
    /// Forces the whole statement to be on one line.
    SameLine,
    /// Forces the expression to be on the next line.
    NextLine,
}

/// If braces should be used or not.
#[derive(Clone, PartialEq, Copy)]
pub enum UseBraces {
    /// Uses braces when the body is on a different line.
    Maintain,
    /// Uses braces if they're used. Doesn't use braces if they're not used.
    WhenNotSingleLine,
    /// Forces the use of braces. Will add them if they aren't used.
    Always,
    /// Forces no braces when when the header is one line and body is one line. Otherwise forces braces.
    PreferNone,
}

/// Whether to use parentheses around a single parameter in an arrow function.
#[derive(Clone, PartialEq, Copy)]
pub enum UseParentheses {
    /// Maintains the current state of the parentheses.
    Maintain,
    /// Forces parentheses.
    Force,
    /// Prefers not using parentheses when possible.
    PreferNone,
}

pub fn resolve_config(config: &HashMap<String, String>) -> TypeScriptConfiguration {
    let mut config = config.clone();
    let semi_colons = get_value(&mut config, "semiColons", true);
    let force_multi_line_arguments = get_value(&mut config, "forceMultiLineArguments", false);
    let force_multi_line_parameters = get_value(&mut config, "forceMultiLineParameters", false);
    let brace_position = get_brace_position(&mut config, "bracePosition", BracePosition::NextLineIfHanging);
    let next_control_flow_position = get_next_control_flow_position(&mut config, "nextControlFlowPosition", NextControlFlowPosition::NextLine);
    let operator_position = get_operator_position(&mut config, "operatorPosition", OperatorPosition::NextLine);
    let single_body_position = get_single_body_position(&mut config, "singleBodyPosition", SingleBodyPosition::Maintain);
    let trailing_commas = get_trailing_commas(&mut config, "trailingCommas", TrailingCommas::Never);
    let use_braces = get_use_braces(&mut config, "useBraces", UseBraces::WhenNotSingleLine);

    let resolved_config = TypeScriptConfiguration {
        new_line_kind: get_new_line_kind(&mut config, "newLineKind", NewLineKind::Auto),
        line_width: get_value(&mut config, "lineWidth", 120),
        use_tabs: get_value(&mut config, "useTabs", false),
        indent_width: get_value(&mut config, "indentWidth", 4),
        single_quotes: get_value(&mut config, "singleQuotes", false),
        /* use parentheses */
        arrow_function_expression_use_parentheses: get_use_parentheses(&mut config, "arrowFunctionExpression.useParentheses", UseParentheses::Maintain),
        /* brace position */
        arrow_function_expression_brace_position: get_brace_position(&mut config, "arrowFunctionExpression.bracePosition", brace_position),
        class_declaration_brace_position: get_brace_position(&mut config, "classDeclaration.bracePosition", brace_position),
        class_expression_brace_position: get_brace_position(&mut config, "classExpression.bracePosition", brace_position),
        constructor_brace_position: get_brace_position(&mut config, "constructor.bracePosition", brace_position),
        do_while_statement_brace_position: get_brace_position(&mut config, "doWhileStatement.bracePosition", brace_position),
        enum_declaration_brace_position: get_brace_position(&mut config, "enumDeclaration.bracePosition", brace_position),
        for_statement_brace_position: get_brace_position(&mut config, "forStatement.bracePosition", brace_position),
        for_in_statement_brace_position: get_brace_position(&mut config, "forInStatement.bracePosition", brace_position),
        for_of_statement_brace_position: get_brace_position(&mut config, "forOfStatement.bracePosition", brace_position),
        get_accessor_brace_position: get_brace_position(&mut config, "getAccessor.bracePosition", brace_position),
        if_statement_brace_position: get_brace_position(&mut config, "ifStatement.bracePosition", brace_position),
        interface_declaration_brace_position: get_brace_position(&mut config, "interfaceDeclaration.bracePosition", brace_position),
        function_declaration_brace_position: get_brace_position(&mut config, "functionDeclaration.bracePosition", brace_position),
        function_expression_brace_position: get_brace_position(&mut config, "functionExpression.bracePosition", brace_position),
        method_brace_position: get_brace_position(&mut config, "method.bracePosition", brace_position),
        module_declaration_brace_position: get_brace_position(&mut config, "moduleDeclaration.bracePosition", brace_position),
        set_accessor_brace_position: get_brace_position(&mut config, "setAccessor.bracePosition", brace_position),
        switch_case_brace_position: get_brace_position(&mut config, "switchCase.bracePosition", brace_position),
        switch_statement_brace_position: get_brace_position(&mut config, "switchStatement.bracePosition", brace_position),
        try_statement_brace_position: get_brace_position(&mut config, "tryStatement.bracePosition", brace_position),
        while_statement_brace_position: get_brace_position(&mut config, "whileStatement.bracePosition", brace_position),
        /* force multi-line arguments */
        call_expression_force_multi_line_arguments: get_value(&mut config, "callExpression.forceMultiLineArguments", force_multi_line_arguments),
        new_expression_force_multi_line_arguments: get_value(&mut config, "newExpression.forceMultiLineArguments", force_multi_line_arguments),
        /* force multi-line parameters */
        arrow_function_expression_force_multi_line_parameters: get_value(&mut config, "arrowFunctionExpression.forceMultiLineParameters", force_multi_line_parameters),
        call_signature_force_multi_line_parameters: get_value(&mut config, "callSignature.forceMultiLineParameters", force_multi_line_parameters),
        construct_signature_force_multi_line_parameters: get_value(&mut config, "constructSignature.forceMultiLineParameters", force_multi_line_parameters),
        constructor_force_multi_line_parameters: get_value(&mut config, "constructor.forceMultiLineParameters", force_multi_line_parameters),
        constructor_type_force_multi_line_parameters: get_value(&mut config, "constructorType.forceMultiLineParameters", force_multi_line_parameters),
        function_declaration_force_multi_line_parameters: get_value(&mut config, "functionDeclaration.forceMultiLineParameters", force_multi_line_parameters),
        function_expression_force_multi_line_parameters: get_value(&mut config, "functionExpression.forceMultiLineParameters", force_multi_line_parameters),
        function_type_force_multi_line_parameters: get_value(&mut config, "functionType.forceMultiLineParameters", force_multi_line_parameters),
        get_accessor_force_multi_line_parameters: get_value(&mut config, "getAccessor.forceMultiLineParameters", force_multi_line_parameters),
        method_force_multi_line_parameters: get_value(&mut config, "method.forceMultiLineParameters", force_multi_line_parameters),
        method_signature_force_multi_line_parameters: get_value(&mut config, "methodSignature.forceMultiLineParameters", force_multi_line_parameters),
        set_accessor_force_multi_line_parameters: get_value(&mut config, "setAccessor.forceMultiLineParameters", force_multi_line_parameters),
        /* member spacing */
        enum_declaration_member_spacing: get_member_spacing(&mut config, "enumDeclaration.memberSpacing", MemberSpacing::Maintain),
        /* next control flow position */
        if_statement_next_control_flow_position: get_next_control_flow_position(&mut config, "ifStatement.nextControlFlowPosition", next_control_flow_position),
        try_statement_next_control_flow_position: get_next_control_flow_position(&mut config, "tryStatement.nextControlFlowPosition", next_control_flow_position),
        /* operator position */
        binary_expression_operator_position: get_operator_position(&mut config, "binaryExpression.operatorPosition", operator_position),
        conditional_expression_operator_position: get_operator_position(&mut config, "conditionalExpression.operatorPosition", operator_position),
        /* semi-colon */
        break_statement_semi_colon: get_value(&mut config, "breakStatement.semiColon", semi_colons),
        call_signature_semi_colon: get_value(&mut config, "callSignature.semiColon", semi_colons),
        class_property_semi_colon: get_value(&mut config, "classProperty.semiColon", semi_colons),
        construct_signature_semi_colon: get_value(&mut config, "constructSignature.semiColon", semi_colons),
        constructor_semi_colon: get_value(&mut config, "constructor.semiColon", semi_colons),
        continue_statement_semi_colon: get_value(&mut config, "continueStatement.semiColon", semi_colons),
        debugger_statement_semi_colon: get_value(&mut config, "debuggerStatement.semiColon", semi_colons),
        do_while_statement_semi_colon: get_value(&mut config, "doWhileStatement.semiColon", semi_colons),
        empty_statement_semi_colon: get_value(&mut config, "emptyStatement.semiColon", semi_colons),
        export_all_declaration_semi_colon: get_value(&mut config, "exportAllDeclaration.semiColon", semi_colons),
        export_assignment_semi_colon: get_value(&mut config, "exportAssignment.semiColon", semi_colons),
        export_default_expression_semi_colon: get_value(&mut config, "exportDefaultExpression.semiColon", semi_colons),
        export_named_declaration_semi_colon: get_value(&mut config, "exportNamedDeclaration.semiColon", semi_colons),
        expression_statement_semi_colon: get_value(&mut config, "expressionStatement.semiColon", semi_colons),
        function_declaration_semi_colon: get_value(&mut config, "functionDeclaration.semiColon", semi_colons),
        get_accessor_semi_colon: get_value(&mut config, "getAccessor.semiColon", semi_colons),
        import_declaration_semi_colon: get_value(&mut config, "importDeclaration.semiColon", semi_colons),
        import_equals_semi_colon: get_value(&mut config, "importEqualsDeclaration.semiColon", semi_colons),
        index_signature_semi_colon: get_value(&mut config, "indexSignature.semiColon", semi_colons),
        mapped_type_semi_colon: get_value(&mut config, "mappedType.semiColon", semi_colons),
        method_semi_colon: get_value(&mut config, "method.semiColon", semi_colons),
        method_signature_semi_colon: get_value(&mut config, "methodSignature.semiColon", semi_colons),
        module_declaration_semi_colon: get_value(&mut config, "moduleDeclaration.semiColon", semi_colons),
        namespace_export_declaration_semi_colon: get_value(&mut config, "namespaceExportDeclaration.semiColon", semi_colons),
        property_signature_semi_colon: get_value(&mut config, "propertySignature.semiColon", semi_colons),
        return_statement_semi_colon: get_value(&mut config, "returnStatement.semiColon", semi_colons),
        set_accessor_semi_colon: get_value(&mut config, "setAccessor.semiColon", semi_colons),
        throw_statement_semi_colon: get_value(&mut config, "throwStatement.semiColon", semi_colons),
        type_alias_semi_colon: get_value(&mut config, "typeAlias.semiColon", semi_colons),
        variable_statement_semi_colon: get_value(&mut config, "variableStatement.semiColon", semi_colons),
        /* single body position */
        if_statement_single_body_position: get_single_body_position(&mut config, "ifStatement.singleBodyPosition", single_body_position),
        for_statement_single_body_position: get_single_body_position(&mut config, "forStatement.singleBodyPosition", single_body_position),
        for_in_statement_single_body_position: get_single_body_position(&mut config, "forInStatement.singleBodyPosition", single_body_position),
        for_of_statement_single_body_position: get_single_body_position(&mut config, "forOfStatement.singleBodyPosition", single_body_position),
        while_statement_single_body_position: get_single_body_position(&mut config, "whileStatement.singleBodyPosition", single_body_position),
        /* trailing commas */
        array_expression_trailing_commas: get_trailing_commas(&mut config, "arrayExpression.trailingCommas", trailing_commas),
        array_pattern_trailing_commas: get_trailing_commas(&mut config, "arrayPattern.trailingCommas", trailing_commas),
        enum_declaration_trailing_commas: get_trailing_commas(&mut config, "enumDeclaration.trailingCommas", trailing_commas),
        object_expression_trailing_commas: get_trailing_commas(&mut config, "objectExpression.trailingCommas", trailing_commas),
        tuple_type_trailing_commas: get_trailing_commas(&mut config, "tupleType.trailingCommas", trailing_commas),
        /* use braces */
        if_statement_use_braces: get_use_braces(&mut config, "ifStatement.useBraces", use_braces),
        for_statement_use_braces: get_use_braces(&mut config, "forStatement.useBraces", use_braces),
        for_in_statement_use_braces: get_use_braces(&mut config, "forInStatement.useBraces", use_braces),
        for_of_statement_use_braces: get_use_braces(&mut config, "forOfStatement.useBraces", use_braces),
        while_statement_use_braces: get_use_braces(&mut config, "whileStatement.useBraces", use_braces),
        /* space settings */
        binary_expression_space_surrounding_bitwise_and_arithmetic_operator: get_value(&mut config, "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator", true),
        construct_signature_space_after_new_keyword: get_value(&mut config, "constructSignature.spaceAfterNewKeyword", false),
        constructor_space_before_parentheses: get_value(&mut config, "constructor.spaceBeforeParentheses", false),
        constructor_type_space_after_new_keyword: get_value(&mut config, "constructorType.spaceAfterNewKeyword", false),
        do_while_statement_space_after_while_keyword: get_value(&mut config, "doWhileStatement.spaceAfterWhileKeyword", true),
        export_declaration_space_surrounding_named_exports: get_value(&mut config, "exportDeclaration.spaceSurroundingNamedExports", true),
        for_statement_space_after_for_keyword: get_value(&mut config, "forStatement.spaceAfterForKeyword", true),
        for_statement_space_after_semi_colons: get_value(&mut config, "forStatement.spaceAfterSemiColons", true),
        for_in_statement_space_after_for_keyword: get_value(&mut config, "forInStatement.spaceAfterForKeyword", true),
        for_of_statement_space_after_for_keyword: get_value(&mut config, "forOfStatement.spaceAfterForKeyword", true),
        function_declaration_space_before_parentheses: get_value(&mut config, "functionDeclaration.spaceBeforeParentheses", false),
        function_expression_space_before_parentheses: get_value(&mut config, "functionExpression.spaceBeforeParentheses", false),
        get_accessor_space_before_parentheses: get_value(&mut config, "getAccessor.spaceBeforeParentheses", false),
        if_statement_space_after_if_keyword: get_value(&mut config, "ifStatement.spaceAfterIfKeyword", true),
        import_declaration_space_surrounding_named_imports: get_value(&mut config, "importDeclaration.spaceSurroundingNamedImports", true),
        jsx_expression_container_space_surrounding_expression: get_value(&mut config, "jsxExpressionContainer.spaceSurroundingExpression", false),
        method_space_before_parentheses: get_value(&mut config, "method.spaceBeforeParentheses", false),
        set_accessor_space_before_parentheses: get_value(&mut config, "setAccessor.spaceBeforeParentheses", false),
        type_annotation_space_before_colon: get_value(&mut config, "typeAnnotation.spaceBeforeColon", false),
        type_assertion_space_before_expression: get_value(&mut config, "typeAssertion.spaceBeforeExpression", true),
        while_statement_space_after_while_keyword: get_value(&mut config, "whileStatement.spaceAfterWhileKeyword", true),
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

// todo: make the functions below more generic (implement FromStr?)

fn get_new_line_kind(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: NewLineKind
) -> NewLineKind {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "auto" => NewLineKind::Auto,
            "\n" => NewLineKind::Unix,
            "\r\n" => NewLineKind::Windows,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

fn get_trailing_commas(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: TrailingCommas
) -> TrailingCommas {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "always" => TrailingCommas::Always,
            "never" => TrailingCommas::Never,
            "onlyMultiLine" => TrailingCommas::OnlyMultiLine,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

fn get_brace_position(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: BracePosition
) -> BracePosition {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => BracePosition::Maintain,
            "sameLine" => BracePosition::SameLine,
            "nextLine" => BracePosition::NextLine,
            "nextLineIfHanging" => BracePosition::NextLineIfHanging,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

fn get_member_spacing(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: MemberSpacing
) -> MemberSpacing {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => MemberSpacing::Maintain,
            "blankline" => MemberSpacing::BlankLine,
            "newline" => MemberSpacing::NewLine,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

fn get_next_control_flow_position(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: NextControlFlowPosition
) -> NextControlFlowPosition {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => NextControlFlowPosition::Maintain,
            "sameLine" => NextControlFlowPosition::SameLine,
            "nextLine" => NextControlFlowPosition::NextLine,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

fn get_operator_position(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: OperatorPosition
) -> OperatorPosition {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => OperatorPosition::Maintain,
            "sameLine" => OperatorPosition::SameLine,
            "nextLine" => OperatorPosition::NextLine,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

fn get_single_body_position(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: SingleBodyPosition
) -> SingleBodyPosition {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => SingleBodyPosition::Maintain,
            "sameLine" => SingleBodyPosition::SameLine,
            "nextLine" => SingleBodyPosition::NextLine,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

fn get_use_braces(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: UseBraces
) -> UseBraces {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => UseBraces::Maintain,
            "whenNotSingleLine" => UseBraces::WhenNotSingleLine,
            "always" => UseBraces::Always,
            "preferNone" => UseBraces::PreferNone,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

fn get_use_parentheses(
    config: &mut HashMap<String, String>,
    prop: &str,
    default_value: UseParentheses
) -> UseParentheses {
    let value = config.get(prop).map(|x| x.parse::<String>().unwrap());
    config.remove(prop);
    if let Some(value) = value {
        match value.as_ref() {
            "maintain" => UseParentheses::Maintain,
            "force" => UseParentheses::Force,
            "preferNone" => UseParentheses::PreferNone,
            "" => default_value,
            _ => panic!("Invalid configuration option {}.", value) // todo: diagnostics instead
        }
    } else {
        default_value
    }
}

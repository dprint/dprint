use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseConfigurationError(String);

impl std::fmt::Display for ParseConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format!("Found invalid value '{}'.", self.0).fmt(f)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeScriptConfiguration {
    pub new_line_kind: NewLineKind,
    pub single_quotes: bool,
    pub line_width: u32,
    pub use_tabs: bool,
    pub indent_width: u8,
    /* use parentheses */
    #[serde(rename = "arrowFunctionExpression.useParentheses")]
    pub arrow_function_expression_use_parentheses: UseParentheses,
    /* brace position */
    #[serde(rename = "arrowFunctionExpression.bracePosition")]
    pub arrow_function_expression_brace_position: BracePosition,
    #[serde(rename = "classDeclaration.bracePosition")]
    pub class_declaration_brace_position: BracePosition,
    #[serde(rename = "classExpression.bracePosition")]
    pub class_expression_brace_position: BracePosition,
    #[serde(rename = "constructor.bracePosition")]
    pub constructor_brace_position: BracePosition,
    #[serde(rename = "doWhileStatement.bracePosition")]
    pub do_while_statement_brace_position: BracePosition,
    #[serde(rename = "enumDeclaration.bracePosition")]
    pub enum_declaration_brace_position: BracePosition,
    #[serde(rename = "getAccessor.bracePosition")]
    pub get_accessor_brace_position: BracePosition,
    #[serde(rename = "ifStatement.bracePosition")]
    pub if_statement_brace_position: BracePosition,
    #[serde(rename = "interfaceDeclaration.bracePosition")]
    pub interface_declaration_brace_position: BracePosition,
    #[serde(rename = "forStatement.bracePosition")]
    pub for_statement_brace_position: BracePosition,
    #[serde(rename = "forInStatement.bracePosition")]
    pub for_in_statement_brace_position: BracePosition,
    #[serde(rename = "forOfStatement.bracePosition")]
    pub for_of_statement_brace_position: BracePosition,
    #[serde(rename = "functionDeclaration.bracePosition")]
    pub function_declaration_brace_position: BracePosition,
    #[serde(rename = "functionExpression.bracePosition")]
    pub function_expression_brace_position: BracePosition,
    #[serde(rename = "method.bracePosition")]
    pub method_brace_position: BracePosition,
    #[serde(rename = "moduleDeclaration.bracePosition")]
    pub module_declaration_brace_position: BracePosition,
    #[serde(rename = "setAccessor.bracePosition")]
    pub set_accessor_brace_position: BracePosition,
    #[serde(rename = "switchCase.bracePosition")]
    pub switch_case_brace_position: BracePosition,
    #[serde(rename = "switchStatement.bracePosition")]
    pub switch_statement_brace_position: BracePosition,
    #[serde(rename = "tryStatement.bracePosition")]
    pub try_statement_brace_position: BracePosition,
    #[serde(rename = "whileStatement.bracePosition")]
    pub while_statement_brace_position: BracePosition,
    /* force multi-line arguments */
    #[serde(rename = "callExpression.forceMultiLineArguments")]
    pub call_expression_force_multi_line_arguments: bool,
    #[serde(rename = "newExpression.forceMultiLineArguments")]
    pub new_expression_force_multi_line_arguments: bool,
    /* force multi-line parameters */
    #[serde(rename = "arrowFunctionExpression.forceMultiLineParameters")]
    pub arrow_function_expression_force_multi_line_parameters: bool,
    #[serde(rename = "callSignature.forceMultiLineParameters")]
    pub call_signature_force_multi_line_parameters: bool,
    #[serde(rename = "constructSignature.forceMultiLineParameters")]
    pub construct_signature_force_multi_line_parameters: bool,
    #[serde(rename = "constructor.forceMultiLineParameters")]
    pub constructor_force_multi_line_parameters: bool,
    #[serde(rename = "constructorType.forceMultiLineParameters")]
    pub constructor_type_force_multi_line_parameters: bool,
    #[serde(rename = "functionDeclaration.forceMultiLineParameters")]
    pub function_declaration_force_multi_line_parameters: bool,
    #[serde(rename = "functionExpression.forceMultiLineParameters")]
    pub function_expression_force_multi_line_parameters: bool,
    #[serde(rename = "functionType.forceMultiLineParameters")]
    pub function_type_force_multi_line_parameters: bool,
    #[serde(rename = "getAccessor.forceMultiLineParameters")]
    pub get_accessor_force_multi_line_parameters: bool,
    #[serde(rename = "method.forceMultiLineParameters")]
    pub method_force_multi_line_parameters: bool,
    #[serde(rename = "methodSignature.forceMultiLineParameters")]
    pub method_signature_force_multi_line_parameters: bool,
    #[serde(rename = "setAccessor.forceMultiLineParameters")]
    pub set_accessor_force_multi_line_parameters: bool,
    /* member spacing */
    #[serde(rename = "enumDeclaration.memberSpacing")]
    pub enum_declaration_member_spacing: MemberSpacing,
    /* next control flow position */
    #[serde(rename = "ifStatement.nextControlFlowPosition")]
    pub if_statement_next_control_flow_position: NextControlFlowPosition,
    #[serde(rename = "tryStatement.nextControlFlowPosition")]
    pub try_statement_next_control_flow_position: NextControlFlowPosition,
    /* operator position */
    #[serde(rename = "binaryExpression.operatorPosition")]
    pub binary_expression_operator_position: OperatorPosition,
    #[serde(rename = "conditionalExpression.operatorPosition")]
    pub conditional_expression_operator_position: OperatorPosition,
    /* semi-colon */
    #[serde(rename = "breakStatement.semiColon")]
    pub break_statement_semi_colon: bool,
    #[serde(rename = "callSignature.semiColon")]
    pub call_signature_semi_colon: bool,
    #[serde(rename = "classProperty.semiColon")]
    pub class_property_semi_colon: bool,
    #[serde(rename = "constructSignature.semiColon")]
    pub construct_signature_semi_colon: bool,
    #[serde(rename = "constructor.semiColon")]
    pub constructor_semi_colon: bool,
    #[serde(rename = "continueStatement.semiColon")]
    pub continue_statement_semi_colon: bool,
    #[serde(rename = "debuggerStatement.semiColon")]
    pub debugger_statement_semi_colon: bool,
    #[serde(rename = "doWhileStatement.semiColon")]
    pub do_while_statement_semi_colon: bool,
    #[serde(rename = "exportAllDeclaration.semiColon")]
    pub export_all_declaration_semi_colon: bool,
    #[serde(rename = "exportAssignment.semiColon")]
    pub export_assignment_semi_colon: bool,
    #[serde(rename = "exportDefaultExpression.semiColon")]
    pub export_default_expression_semi_colon: bool,
    #[serde(rename = "exportNamedDeclaration.semiColon")]
    pub export_named_declaration_semi_colon: bool,
    #[serde(rename = "expressionStatement.semiColon")]
    pub expression_statement_semi_colon: bool,
    #[serde(rename = "functionDeclaration.semiColon")]
    pub function_declaration_semi_colon: bool,
    #[serde(rename = "getAccessor.semiColon")]
    pub get_accessor_semi_colon: bool,
    #[serde(rename = "importDeclaration.semiColon")]
    pub import_declaration_semi_colon: bool,
    #[serde(rename = "importEqualsDeclaration.semiColon")]
    pub import_equals_declaration_semi_colon: bool,
    #[serde(rename = "indexSignature.semiColon")]
    pub index_signature_semi_colon: bool,
    #[serde(rename = "mappedType.semiColon")]
    pub mapped_type_semi_colon: bool,
    #[serde(rename = "method.semiColon")]
    pub method_semi_colon: bool,
    #[serde(rename = "methodSignature.semiColon")]
    pub method_signature_semi_colon: bool,
    #[serde(rename = "moduleDeclaration.semiColon")]
    pub module_declaration_semi_colon: bool,
    #[serde(rename = "namespaceExportDeclaration.semiColon")]
    pub namespace_export_declaration_semi_colon: bool,
    #[serde(rename = "propertySignature.semiColon")]
    pub property_signature_semi_colon: bool,
    #[serde(rename = "returnStatement.semiColon")]
    pub return_statement_semi_colon: bool,
    #[serde(rename = "setAccessor.semiColon")]
    pub set_accessor_semi_colon: bool,
    #[serde(rename = "throwStatement.semiColon")]
    pub throw_statement_semi_colon: bool,
    #[serde(rename = "typeAlias.semiColon")]
    pub type_alias_semi_colon: bool,
    #[serde(rename = "variableStatement.semiColon")]
    pub variable_statement_semi_colon: bool,
    /* single body position */
    #[serde(rename = "ifStatement.singleBodyPosition")]
    pub if_statement_single_body_position: SingleBodyPosition,
    #[serde(rename = "forStatement.singleBodyPosition")]
    pub for_statement_single_body_position: SingleBodyPosition,
    #[serde(rename = "forInStatement.singleBodyPosition")]
    pub for_in_statement_single_body_position: SingleBodyPosition,
    #[serde(rename = "forOfStatement.singleBodyPosition")]
    pub for_of_statement_single_body_position: SingleBodyPosition,
    #[serde(rename = "whileStatement.singleBodyPosition")]
    pub while_statement_single_body_position: SingleBodyPosition,
    /* trailing commas */
    #[serde(rename = "arrayExpression.trailingCommas")]
    pub array_expression_trailing_commas: TrailingCommas,
    #[serde(rename = "arrayPattern.trailingCommas")]
    pub array_pattern_trailing_commas: TrailingCommas,
    #[serde(rename = "enumDeclaration.trailingCommas")]
    pub enum_declaration_trailing_commas: TrailingCommas,
    #[serde(rename = "objectExpression.trailingCommas")]
    pub object_expression_trailing_commas: TrailingCommas,
    #[serde(rename = "tupleType.trailingCommas")]
    pub tuple_type_trailing_commas: TrailingCommas,
    /* use braces */
    #[serde(rename = "ifStatement.useBraces")]
    pub if_statement_use_braces: UseBraces,
    #[serde(rename = "forStatement.useBraces")]
    pub for_statement_use_braces: UseBraces,
    #[serde(rename = "forOfStatement.useBraces")]
    pub for_of_statement_use_braces: UseBraces,
    #[serde(rename = "forInStatement.useBraces")]
    pub for_in_statement_use_braces: UseBraces,
    #[serde(rename = "whileStatement.useBraces")]
    pub while_statement_use_braces: UseBraces,

    /* use space separator */

    /// Whether to surround bitwise and arithmetic operators in a binary expression with spaces.
    /// * `true` (default) - Ex. `1 + 2`
    /// * `false` - Ex. `1+2`
    #[serde(rename = "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator")]
    pub binary_expression_space_surrounding_bitwise_and_arithmetic_operator: bool,
    /// Whether to add a space after the `new` keyword in a construct signature.
    /// `true` - Ex. `new (): MyClass;`
    /// `false` (default) - Ex. `new(): MyClass;`
    #[serde(rename = "constructSignature.spaceAfterNewKeyword")]
    pub construct_signature_space_after_new_keyword: bool,
    /// Whether to add a space before the parentheses of a constructor.
    /// `true` - Ex. `constructor ()`
    /// `false` (false) - Ex. `constructor()`
    #[serde(rename = "constructor.spaceBeforeParentheses")]
    pub constructor_space_before_parentheses: bool,
    /// Whether to add a space after the `new` keyword in a constructor type.
    /// `true` - Ex. `type MyClassCtor = new () => MyClass;`
    /// `false` (default) - Ex. `type MyClassCtor = new() => MyClass;`
    #[serde(rename = "constructorType.spaceAfterNewKeyword")]
    pub constructor_type_space_after_new_keyword: bool,
    /// Whether to add a space after the `while` keyword in a do while statement.
    /// `true` (true) - Ex. `do {\n} while (condition);`
    /// `false` - Ex. `do {\n} while(condition);`
    #[serde(rename = "doWhileStatement.spaceAfterWhileKeyword")]
    pub do_while_statement_space_after_while_keyword: bool,
    /// Whether to add spaces around named exports in an export declaration.
    /// * `true` (default) - Ex. `export { SomeExport, OtherExport };`
    /// * `false` - Ex. `export {SomeExport, OtherExport};`
    #[serde(rename = "exportDeclarationSpace.surroundingNamedExports")]
    pub export_declaration_space_surrounding_named_exports: bool,
    /// Whether to add a space after the `for` keyword in a "for" statement.
    /// * `true` (default) - Ex. `for (let i = 0; i < 5; i++)`
    /// * `false` - Ex. `for(let i = 0; i < 5; i++)`
    #[serde(rename = "forStatement.spaceAfterForKeyword")]
    pub for_statement_space_after_for_keyword: bool,
    /// Whether to add a space after the semi-colons in a "for" statement.
    /// * `true` (default) - Ex. `for (let i = 0; i < 5; i++)`
    /// * `false` - Ex. `for (let i = 0;i < 5;i++)`
    #[serde(rename = "forStatement.spaceAfterSemiColons")]
    pub for_statement_space_after_semi_colons: bool,
    /// Whether to add a space after the `for` keyword in a "for in" statement.
    /// * `true` (default) - Ex. `for (const prop in obj)`
    /// * `false` - Ex. `for(const prop in obj)`
    #[serde(rename = "forInStatement.spaceAfterForKeyword")]
    pub for_in_statement_space_after_for_keyword: bool,
    /// Whether to add a space after the `for` keyword in a "for of" statement.
    /// * `true` (default) - Ex. `for (const value of myArray)`
    /// * `false` - Ex. `for(const value of myArray)`
    #[serde(rename = "forOfStatement.spaceAfterForKeyword")]
    pub for_of_statement_space_after_for_keyword: bool,
    /// Whether to add a space before the parentheses of a function declaration.
    /// * `true` - Ex. `function myFunction ()`
    /// * `false` (default) - Ex. `function myFunction()`
    #[serde(rename = "functionDeclaration.spaceBeforeParentheses")]
    pub function_declaration_space_before_parentheses: bool,
    /// Whether to add a space before the parentheses of a function expression.
    /// `true` - Ex. `function ()`
    /// `false` (default) - Ex. `function()`
    #[serde(rename = "functionExpression.spaceBeforeParentheses")]
    pub function_expression_space_before_parentheses: bool,
    /// Whether to add a space before the parentheses of a get accessor.
    /// `true` - Ex. `get myProp ()`
    /// `false` (false) - Ex. `get myProp()`
    #[serde(rename = "getAccessor.spaceBeforeParentheses")]
    pub get_accessor_space_before_parentheses: bool,
    /// Whether to add a space after the `if` keyword in an "if" statement.
    /// `true` (default) - Ex. `if (true)`
    /// `false` - Ex. `if(true)`
    #[serde(rename = "ifStatement.spaceAfterIfKeyword")]
    pub if_statement_space_after_if_keyword: bool,
    /// Whether to add spaces around named imports in an import declaration.
    /// * `true` (default) - Ex. `import { SomeExport, OtherExport } from "my-module";`
    /// * `false` - Ex. `import {SomeExport, OtherExport} from "my-module";`
    #[serde(rename = "importDeclaration.spaceSurroundingNamedImports")]
    pub import_declaration_space_surrounding_named_imports: bool,
    /// Whether to add a space surrounding the expression of a JSX container.
    /// * `true` - Ex. `{ myValue }`
    /// * `false` (default) - Ex. `{myValue}`
    #[serde(rename = "jsxExpressionContainer.spaceSurroundingExpression")]
    pub jsx_expression_container_space_surrounding_expression: bool,
    /// Whether to add a space before the parentheses of a method.
    /// `true` - Ex. `myMethod ()`
    /// `false` - Ex. `myMethod()`
    #[serde(rename = "method.spaceBeforeParentheses")]
    pub method_space_before_parentheses: bool,
    /// Whether to add a space before the parentheses of a set accessor.
    /// `true` - Ex. `set myProp (value: string)`
    /// `false` (default) - Ex. `set myProp(value: string)`
    #[serde(rename = "setAccessor.spaceBeforeParentheses")]
    pub set_accessor_space_before_parentheses: bool,
    /// Whether to add a space before the colon of a type annotation.
    /// * `true` - Ex. `function myFunction() : string`
    /// * `false` (default) - Ex. `function myFunction(): string`
    #[serde(rename = "typeAnnotation.spaceBeforeColon")]
    pub type_annotation_space_before_colon: bool,
    /// Whether to add a space before the expression in a type assertion.
    /// * `true` (default) - Ex. `<string> myValue`
    /// * `false` - Ex. `<string>myValue`
    #[serde(rename = "typeAssertion.spaceBeforeExpression")]
    pub type_assertion_space_before_expression: bool,
    /// Whether to add a space after the `while` keyword in a while statement.
    /// * `true` (default) - Ex. `while (true)`
    /// * `false` - Ex. `while(true)`
    #[serde(rename = "whileStatement.spaceAfterWhileKeyword")]
    pub while_statement_space_after_while_keyword: bool,
}

// todo: maybe move NewLineKind to core? and then maybe re-export it here?
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
pub enum NewLineKind {
    /// Decide which newline kind to use based on the last newline in the file.
    #[serde(rename = "auto")]
    Auto,
    /// Use slash n new lines.
    #[serde(rename = "\n")]
    Unix,
    /// Use slash r slash n new lines.
    #[serde(rename = "\r\n")]
    Windows,
}

impl std::str::FromStr for NewLineKind {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(NewLineKind::Auto),
            "\n" => Ok(NewLineKind::Unix),
            "\r\n" => Ok(NewLineKind::Windows),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// Trailing comma possibilities.
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrailingCommas {
    /// Trailing commas should not be used.
    Never,
    /// Trailing commas should always be used.
    Always,
    /// Trailing commas should only be used in multi-line scenarios.
    OnlyMultiLine,
}

impl std::str::FromStr for TrailingCommas {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "always" => Ok(TrailingCommas::Always),
            "never" => Ok(TrailingCommas::Never),
            "onlyMultiLine" => Ok(TrailingCommas::OnlyMultiLine),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// Where to place the opening brace.
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

impl std::str::FromStr for BracePosition {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "maintain" => Ok(BracePosition::Maintain),
            "sameLine" => Ok(BracePosition::SameLine),
            "nextLine" => Ok(BracePosition::NextLine),
            "nextLineIfHanging" => Ok(BracePosition::NextLineIfHanging),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// How to space members.
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemberSpacing {
    /// Maintains whether a newline or blankline is used.
    Maintain,
    /// Forces a new line between members.
    #[serde(rename = "newline")]
    NewLine,
    /// Forces a blank line between members.
    #[serde(rename = "blankline")]
    BlankLine,
}

impl std::str::FromStr for MemberSpacing {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "maintain" => Ok(MemberSpacing::Maintain),
            "blankline" => Ok(MemberSpacing::BlankLine),
            "newline" => Ok(MemberSpacing::NewLine),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// Where to place the next control flow within a control flow statement.
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NextControlFlowPosition {
    /// Maintains the next control flow being on the next line or the same line.
    Maintain,
    /// Forces the next control flow to be on the same line.
    SameLine,
    /// Forces the next control flow to be on the next line.
    NextLine,
}

impl std::str::FromStr for NextControlFlowPosition {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "maintain" => Ok(NextControlFlowPosition::Maintain),
            "sameLine" => Ok(NextControlFlowPosition::SameLine),
            "nextLine" => Ok(NextControlFlowPosition::NextLine),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// Where to place the operator for expressions that span multiple lines.
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperatorPosition {
    /// Maintains the operator being on the next line or the same line.
    Maintain,
    /// Forces the operator to be on the same line.
    SameLine,
    /// Forces the operator to be on the next line.
    NextLine,
}

impl std::str::FromStr for OperatorPosition {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "maintain" => Ok(OperatorPosition::Maintain),
            "sameLine" => Ok(OperatorPosition::SameLine),
            "nextLine" => Ok(OperatorPosition::NextLine),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// Where to place the expression of a statement that could possibly be on one line (ex. `if (true) console.log(5);`).
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SingleBodyPosition {
    /// Maintains the position of the expression.
    Maintain,
    /// Forces the whole statement to be on one line.
    SameLine,
    /// Forces the expression to be on the next line.
    NextLine,
}

impl std::str::FromStr for SingleBodyPosition {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "maintain" => Ok(SingleBodyPosition::Maintain),
            "sameLine" => Ok(SingleBodyPosition::SameLine),
            "nextLine" => Ok(SingleBodyPosition::NextLine),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// If braces should be used or not.
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

impl std::str::FromStr for UseBraces {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "maintain" => Ok(UseBraces::Maintain),
            "whenNotSingleLine" => Ok(UseBraces::WhenNotSingleLine),
            "always" => Ok(UseBraces::Always),
            "preferNone" => Ok(UseBraces::PreferNone),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// Whether to use parentheses around a single parameter in an arrow function.
#[derive(Clone, PartialEq, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UseParentheses {
    /// Maintains the current state of the parentheses.
    Maintain,
    /// Forces parentheses.
    Force,
    /// Prefers not using parentheses when possible.
    PreferNone,
}

impl std::str::FromStr for UseParentheses {
    type Err = ParseConfigurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "maintain" => Ok(UseParentheses::Maintain),
            "force" => Ok(UseParentheses::Force),
            "preferNone" => Ok(UseParentheses::PreferNone),
            _ => Err(ParseConfigurationError(String::from(s))),
        }
    }
}

/// Represents a problem within the configuration.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationDiagnostic {
    /// The property name the problem occurred on.
    pub property_name: String,
    /// The diagnostic message that should be displayed to the user
    pub message: String,
}

pub fn resolve_config(config: &HashMap<String, String>, diagnostics: &mut Vec<ConfigurationDiagnostic>) -> TypeScriptConfiguration {
    let mut config = config.clone();
    let semi_colons = get_value(&mut config, "semiColons", true, diagnostics);
    let force_multi_line_arguments = get_value(&mut config, "forceMultiLineArguments", false, diagnostics);
    let force_multi_line_parameters = get_value(&mut config, "forceMultiLineParameters", false, diagnostics);
    let brace_position = get_value(&mut config, "bracePosition", BracePosition::NextLineIfHanging, diagnostics);
    let next_control_flow_position = get_value(&mut config, "nextControlFlowPosition", NextControlFlowPosition::NextLine, diagnostics);
    let operator_position = get_value(&mut config, "operatorPosition", OperatorPosition::NextLine, diagnostics);
    let single_body_position = get_value(&mut config, "singleBodyPosition", SingleBodyPosition::Maintain, diagnostics);
    let trailing_commas = get_value(&mut config, "trailingCommas", TrailingCommas::Never, diagnostics);
    let use_braces = get_value(&mut config, "useBraces", UseBraces::WhenNotSingleLine, diagnostics);

    let resolved_config = TypeScriptConfiguration {
        new_line_kind: get_value(&mut config, "newLineKind", NewLineKind::Auto, diagnostics),
        line_width: get_value(&mut config, "lineWidth", 120, diagnostics),
        use_tabs: get_value(&mut config, "useTabs", false, diagnostics),
        indent_width: get_value(&mut config, "indentWidth", 4, diagnostics),
        single_quotes: get_value(&mut config, "singleQuotes", false, diagnostics),
        /* use parentheses */
        arrow_function_expression_use_parentheses: get_value(&mut config, "arrowFunctionExpression.useParentheses", UseParentheses::Maintain, diagnostics),
        /* brace position */
        arrow_function_expression_brace_position: get_value(&mut config, "arrowFunctionExpression.bracePosition", brace_position, diagnostics),
        class_declaration_brace_position: get_value(&mut config, "classDeclaration.bracePosition", brace_position, diagnostics),
        class_expression_brace_position: get_value(&mut config, "classExpression.bracePosition", brace_position, diagnostics),
        constructor_brace_position: get_value(&mut config, "constructor.bracePosition", brace_position, diagnostics),
        do_while_statement_brace_position: get_value(&mut config, "doWhileStatement.bracePosition", brace_position, diagnostics),
        enum_declaration_brace_position: get_value(&mut config, "enumDeclaration.bracePosition", brace_position, diagnostics),
        for_statement_brace_position: get_value(&mut config, "forStatement.bracePosition", brace_position, diagnostics),
        for_in_statement_brace_position: get_value(&mut config, "forInStatement.bracePosition", brace_position, diagnostics),
        for_of_statement_brace_position: get_value(&mut config, "forOfStatement.bracePosition", brace_position, diagnostics),
        get_accessor_brace_position: get_value(&mut config, "getAccessor.bracePosition", brace_position, diagnostics),
        if_statement_brace_position: get_value(&mut config, "ifStatement.bracePosition", brace_position, diagnostics),
        interface_declaration_brace_position: get_value(&mut config, "interfaceDeclaration.bracePosition", brace_position, diagnostics),
        function_declaration_brace_position: get_value(&mut config, "functionDeclaration.bracePosition", brace_position, diagnostics),
        function_expression_brace_position: get_value(&mut config, "functionExpression.bracePosition", brace_position, diagnostics),
        method_brace_position: get_value(&mut config, "method.bracePosition", brace_position, diagnostics),
        module_declaration_brace_position: get_value(&mut config, "moduleDeclaration.bracePosition", brace_position, diagnostics),
        set_accessor_brace_position: get_value(&mut config, "setAccessor.bracePosition", brace_position, diagnostics),
        switch_case_brace_position: get_value(&mut config, "switchCase.bracePosition", brace_position, diagnostics),
        switch_statement_brace_position: get_value(&mut config, "switchStatement.bracePosition", brace_position, diagnostics),
        try_statement_brace_position: get_value(&mut config, "tryStatement.bracePosition", brace_position, diagnostics),
        while_statement_brace_position: get_value(&mut config, "whileStatement.bracePosition", brace_position, diagnostics),
        /* force multi-line arguments */
        call_expression_force_multi_line_arguments: get_value(&mut config, "callExpression.forceMultiLineArguments", force_multi_line_arguments, diagnostics),
        new_expression_force_multi_line_arguments: get_value(&mut config, "newExpression.forceMultiLineArguments", force_multi_line_arguments, diagnostics),
        /* force multi-line parameters */
        arrow_function_expression_force_multi_line_parameters: get_value(&mut config, "arrowFunctionExpression.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        call_signature_force_multi_line_parameters: get_value(&mut config, "callSignature.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        construct_signature_force_multi_line_parameters: get_value(&mut config, "constructSignature.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        constructor_force_multi_line_parameters: get_value(&mut config, "constructor.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        constructor_type_force_multi_line_parameters: get_value(&mut config, "constructorType.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        function_declaration_force_multi_line_parameters: get_value(&mut config, "functionDeclaration.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        function_expression_force_multi_line_parameters: get_value(&mut config, "functionExpression.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        function_type_force_multi_line_parameters: get_value(&mut config, "functionType.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        get_accessor_force_multi_line_parameters: get_value(&mut config, "getAccessor.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        method_force_multi_line_parameters: get_value(&mut config, "method.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        method_signature_force_multi_line_parameters: get_value(&mut config, "methodSignature.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        set_accessor_force_multi_line_parameters: get_value(&mut config, "setAccessor.forceMultiLineParameters", force_multi_line_parameters, diagnostics),
        /* member spacing */
        enum_declaration_member_spacing: get_value(&mut config, "enumDeclaration.memberSpacing", MemberSpacing::Maintain, diagnostics),
        /* next control flow position */
        if_statement_next_control_flow_position: get_value(&mut config, "ifStatement.nextControlFlowPosition", next_control_flow_position, diagnostics),
        try_statement_next_control_flow_position: get_value(&mut config, "tryStatement.nextControlFlowPosition", next_control_flow_position, diagnostics),
        /* operator position */
        binary_expression_operator_position: get_value(&mut config, "binaryExpression.operatorPosition", operator_position, diagnostics),
        conditional_expression_operator_position: get_value(&mut config, "conditionalExpression.operatorPosition", operator_position, diagnostics),
        /* semi-colon */
        break_statement_semi_colon: get_value(&mut config, "breakStatement.semiColon", semi_colons, diagnostics),
        call_signature_semi_colon: get_value(&mut config, "callSignature.semiColon", semi_colons, diagnostics),
        class_property_semi_colon: get_value(&mut config, "classProperty.semiColon", semi_colons, diagnostics),
        construct_signature_semi_colon: get_value(&mut config, "constructSignature.semiColon", semi_colons, diagnostics),
        constructor_semi_colon: get_value(&mut config, "constructor.semiColon", semi_colons, diagnostics),
        continue_statement_semi_colon: get_value(&mut config, "continueStatement.semiColon", semi_colons, diagnostics),
        debugger_statement_semi_colon: get_value(&mut config, "debuggerStatement.semiColon", semi_colons, diagnostics),
        do_while_statement_semi_colon: get_value(&mut config, "doWhileStatement.semiColon", semi_colons, diagnostics),
        export_all_declaration_semi_colon: get_value(&mut config, "exportAllDeclaration.semiColon", semi_colons, diagnostics),
        export_assignment_semi_colon: get_value(&mut config, "exportAssignment.semiColon", semi_colons, diagnostics),
        export_default_expression_semi_colon: get_value(&mut config, "exportDefaultExpression.semiColon", semi_colons, diagnostics),
        export_named_declaration_semi_colon: get_value(&mut config, "exportNamedDeclaration.semiColon", semi_colons, diagnostics),
        expression_statement_semi_colon: get_value(&mut config, "expressionStatement.semiColon", semi_colons, diagnostics),
        function_declaration_semi_colon: get_value(&mut config, "functionDeclaration.semiColon", semi_colons, diagnostics),
        get_accessor_semi_colon: get_value(&mut config, "getAccessor.semiColon", semi_colons, diagnostics),
        import_declaration_semi_colon: get_value(&mut config, "importDeclaration.semiColon", semi_colons, diagnostics),
        import_equals_declaration_semi_colon: get_value(&mut config, "importEqualsDeclaration.semiColon", semi_colons, diagnostics),
        index_signature_semi_colon: get_value(&mut config, "indexSignature.semiColon", semi_colons, diagnostics),
        mapped_type_semi_colon: get_value(&mut config, "mappedType.semiColon", semi_colons, diagnostics),
        method_semi_colon: get_value(&mut config, "method.semiColon", semi_colons, diagnostics),
        method_signature_semi_colon: get_value(&mut config, "methodSignature.semiColon", semi_colons, diagnostics),
        module_declaration_semi_colon: get_value(&mut config, "moduleDeclaration.semiColon", semi_colons, diagnostics),
        namespace_export_declaration_semi_colon: get_value(&mut config, "namespaceExportDeclaration.semiColon", semi_colons, diagnostics),
        property_signature_semi_colon: get_value(&mut config, "propertySignature.semiColon", semi_colons, diagnostics),
        return_statement_semi_colon: get_value(&mut config, "returnStatement.semiColon", semi_colons, diagnostics),
        set_accessor_semi_colon: get_value(&mut config, "setAccessor.semiColon", semi_colons, diagnostics),
        throw_statement_semi_colon: get_value(&mut config, "throwStatement.semiColon", semi_colons, diagnostics),
        type_alias_semi_colon: get_value(&mut config, "typeAlias.semiColon", semi_colons, diagnostics),
        variable_statement_semi_colon: get_value(&mut config, "variableStatement.semiColon", semi_colons, diagnostics),
        /* single body position */
        if_statement_single_body_position: get_value(&mut config, "ifStatement.singleBodyPosition", single_body_position, diagnostics),
        for_statement_single_body_position: get_value(&mut config, "forStatement.singleBodyPosition", single_body_position, diagnostics),
        for_in_statement_single_body_position: get_value(&mut config, "forInStatement.singleBodyPosition", single_body_position, diagnostics),
        for_of_statement_single_body_position: get_value(&mut config, "forOfStatement.singleBodyPosition", single_body_position, diagnostics),
        while_statement_single_body_position: get_value(&mut config, "whileStatement.singleBodyPosition", single_body_position, diagnostics),
        /* trailing commas */
        array_expression_trailing_commas: get_value(&mut config, "arrayExpression.trailingCommas", trailing_commas, diagnostics),
        array_pattern_trailing_commas: get_value(&mut config, "arrayPattern.trailingCommas", trailing_commas, diagnostics),
        enum_declaration_trailing_commas: get_value(&mut config, "enumDeclaration.trailingCommas", trailing_commas, diagnostics),
        object_expression_trailing_commas: get_value(&mut config, "objectExpression.trailingCommas", trailing_commas, diagnostics),
        tuple_type_trailing_commas: get_value(&mut config, "tupleType.trailingCommas", trailing_commas, diagnostics),
        /* use braces */
        if_statement_use_braces: get_value(&mut config, "ifStatement.useBraces", use_braces, diagnostics),
        for_statement_use_braces: get_value(&mut config, "forStatement.useBraces", use_braces, diagnostics),
        for_in_statement_use_braces: get_value(&mut config, "forInStatement.useBraces", use_braces, diagnostics),
        for_of_statement_use_braces: get_value(&mut config, "forOfStatement.useBraces", use_braces, diagnostics),
        while_statement_use_braces: get_value(&mut config, "whileStatement.useBraces", use_braces, diagnostics),
        /* space settings */
        binary_expression_space_surrounding_bitwise_and_arithmetic_operator: get_value(&mut config, "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator", true, diagnostics),
        construct_signature_space_after_new_keyword: get_value(&mut config, "constructSignature.spaceAfterNewKeyword", false, diagnostics),
        constructor_space_before_parentheses: get_value(&mut config, "constructor.spaceBeforeParentheses", false, diagnostics),
        constructor_type_space_after_new_keyword: get_value(&mut config, "constructorType.spaceAfterNewKeyword", false, diagnostics),
        do_while_statement_space_after_while_keyword: get_value(&mut config, "doWhileStatement.spaceAfterWhileKeyword", true, diagnostics),
        export_declaration_space_surrounding_named_exports: get_value(&mut config, "exportDeclaration.spaceSurroundingNamedExports", true, diagnostics),
        for_statement_space_after_for_keyword: get_value(&mut config, "forStatement.spaceAfterForKeyword", true, diagnostics),
        for_statement_space_after_semi_colons: get_value(&mut config, "forStatement.spaceAfterSemiColons", true, diagnostics),
        for_in_statement_space_after_for_keyword: get_value(&mut config, "forInStatement.spaceAfterForKeyword", true, diagnostics),
        for_of_statement_space_after_for_keyword: get_value(&mut config, "forOfStatement.spaceAfterForKeyword", true, diagnostics),
        function_declaration_space_before_parentheses: get_value(&mut config, "functionDeclaration.spaceBeforeParentheses", false, diagnostics),
        function_expression_space_before_parentheses: get_value(&mut config, "functionExpression.spaceBeforeParentheses", false, diagnostics),
        get_accessor_space_before_parentheses: get_value(&mut config, "getAccessor.spaceBeforeParentheses", false, diagnostics),
        if_statement_space_after_if_keyword: get_value(&mut config, "ifStatement.spaceAfterIfKeyword", true, diagnostics),
        import_declaration_space_surrounding_named_imports: get_value(&mut config, "importDeclaration.spaceSurroundingNamedImports", true, diagnostics),
        jsx_expression_container_space_surrounding_expression: get_value(&mut config, "jsxExpressionContainer.spaceSurroundingExpression", false, diagnostics),
        method_space_before_parentheses: get_value(&mut config, "method.spaceBeforeParentheses", false, diagnostics),
        set_accessor_space_before_parentheses: get_value(&mut config, "setAccessor.spaceBeforeParentheses", false, diagnostics),
        type_annotation_space_before_colon: get_value(&mut config, "typeAnnotation.spaceBeforeColon", false, diagnostics),
        type_assertion_space_before_expression: get_value(&mut config, "typeAssertion.spaceBeforeExpression", true, diagnostics),
        while_statement_space_after_while_keyword: get_value(&mut config, "whileStatement.spaceAfterWhileKeyword", true, diagnostics),
    };

    for (key, _) in config.iter() {
        diagnostics.push(ConfigurationDiagnostic {
            property_name: String::from(key),
            message: format!("Unexpected property in configuration: {}", key),
        });
    }

    return resolved_config;
}

fn get_value<T>(
    config: &mut HashMap<String, String>,
    prop: &'static str,
    default_value: T,
    diagnostics: &mut Vec<ConfigurationDiagnostic>
) -> T where T : std::str::FromStr, <T as std::str::FromStr>::Err : std::fmt::Display {
    let value = if let Some(raw_value) = config.get(prop) {
        if raw_value.trim() == "" {
            default_value
        } else {
            let parsed_value = raw_value.parse::<T>();
            match parsed_value {
                Ok(parsed_value) => parsed_value,
                Err(message) => {
                    diagnostics.push(ConfigurationDiagnostic {
                        property_name: String::from(prop),
                        message: format!("Error parsing configuration value for '{}'. Message: {}", prop, message)
                    });
                    default_value
                }
            }
        }
    } else {
        default_value
    };
    config.remove(prop);
    return value;
}

// todo: tests, but this is currently tested by the javascript code in dprint-plugin-typescript

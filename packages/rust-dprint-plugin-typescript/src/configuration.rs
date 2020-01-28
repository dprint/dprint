use std::collections::HashMap;
use serde::{Serialize, Deserialize};

// todo: should probably use more macros to reduce the amount of code in here...

/// TypeScript formatting configuration builder.
///
/// # Example
///
/// ```
/// use dprint_plugin_typescript::*;
///
/// let config = TypeScriptConfiguration::new()
///     .line_width(80)
///     .force_multi_line_parameters(true)
///     .force_multi_line_arguments(true)
///     .single_quotes(true)
///     .next_control_flow_position(NextControlFlowPosition::SameLine)
///     .resolve();
/// ```
pub struct TypeScriptConfiguration {
    config: HashMap<String, String>,
}

impl TypeScriptConfiguration {
    /// Constructs a new `TypeScriptConfiguration`
    pub fn new() -> TypeScriptConfiguration {
        TypeScriptConfiguration {
            config: HashMap::new(),
        }
    }

    /// Gets the final configuration that can be used to format a file.
    pub fn resolve(&self) -> ResolvedTypeScriptConfiguration {
        resolve_config(&self.config).config
    }

    /// The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.
    /// Default: 120
    pub fn line_width(&mut self, value: u32) -> &mut Self {
        self.insert("lineWidth", value)
    }

    /// Whether to use tabs (true) or spaces (false).
    /// Default: false
    pub fn use_tabs(&mut self, value: bool) -> &mut Self {
        self.insert("useTabs", value)
    }

    /// The number of columns for an indent.
    /// Default: 4
    pub fn indent_width(&mut self, value: u8) -> &mut Self {
        self.insert("indentWidth", value)
    }

    /// Whether to use single quotes (true) or double quotes (false).
    /// Default: false
    pub fn single_quotes(&mut self, value: bool) -> &mut Self {
        self.insert("singleQuotes", value)
    }

    /// The kind of newline to use.
    /// Default: `NewLineKind::Auto`
    pub fn new_line_kind(&mut self, value: NewLineKind) -> &mut Self {
        self.insert("newLineKind", value)
    }

    /// Whether statements should end in a semi-colon.
    /// Default: true
    pub fn semi_colons(&mut self, value: bool) -> &mut Self {
        self.insert("semiColons", value)
    }

    /// Forces an argument list to be multi-line when it exceeds the line width.
    /// Note: When false, it will be hanging when the first argument is on the same line
    /// as the open parenthesis and multi-line when on a different line.
    /// Default: false
    pub fn force_multi_line_arguments(&mut self, value: bool) -> &mut Self {
        self.insert("forceMultiLineArguments", value)
    }

    /// Forces a parameter list to be multi-line when it exceeds the line width.
    /// Note: When false, it will be hanging when the first parameter is on the same line
    /// as the open parenthesis and multi-line when on a different line.
    /// Default: false
    pub fn force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("forceMultiLineParameters", value)
    }

    /// Where to place the opening brace.
    /// Default: `BracePosition::NextLineIfHanging`
    pub fn brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("bracePosition", value)
    }

    /// Where to place the next control flow within a control flow statement.
    /// Default: `NextControlFlowPosition::NextLine`
    pub fn next_control_flow_position(&mut self, value: NextControlFlowPosition) -> &mut Self {
        self.insert("nextControlFlowPosition", value)
    }

    /// Where to place the operator for expressions that span multiple lines.
    /// Default: `OperatorPosition::NextLine`
    pub fn operator_position(&mut self, value: OperatorPosition) -> &mut Self {
        self.insert("operatorPosition", value)
    }

    /// Where to place the expression of a statement that could possibly be on one line (ex. `if (true) console.log(5);`).
    /// Default: SingleBodyPosition::Maintain
    pub fn single_body_position(&mut self, value: SingleBodyPosition) -> &mut Self {
        self.insert("singleBodyPosition", value)
    }

    /// If trailing commas should be used.
    /// Default: `TrailingCommas::Never`
    pub fn trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("trailingCommas", value)
    }

    /// If braces should be used or not.
    /// Default: `UseBraces::WhenNotSingleLine`
    pub fn use_braces(&mut self, value: UseBraces) -> &mut Self {
        self.insert("useBraces", value)
    }

    /* space settings */

    /// Whether to surround bitwise and arithmetic operators in a binary expression with spaces.
    /// * `true` (default) - Ex. `1 + 2`
    /// * `false` - Ex. `1+2`
    pub fn binary_expression_space_surrounding_bitwise_and_arithmetic_operator(&mut self, value: bool) -> &mut Self {
        self.insert("binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator", value)
    }

    /// Whether to add a space after the `new` keyword in a construct signature.
    /// `true` - Ex. `new (): MyClass;`
    /// `false` (default) - Ex. `new(): MyClass;`
    pub fn construct_signature_space_after_new_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("constructSignature.spaceAfterNewKeyword", value)
    }

    /// Whether to add a space before the parentheses of a constructor.
    /// `true` - Ex. `constructor ()`
    /// `false` (false) - Ex. `constructor()`
    pub fn constructor_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("constructor.spaceBeforeParentheses", value)
    }

    /// Whether to add a space after the `new` keyword in a constructor type.
    /// `true` - Ex. `type MyClassCtor = new () => MyClass;`
    /// `false` (default) - Ex. `type MyClassCtor = new() => MyClass;`
    pub fn constructor_type_space_after_new_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("constructorType.spaceAfterNewKeyword", value)
    }

    /// Whether to add a space after the `while` keyword in a do while statement.
    /// `true` (true) - Ex. `do {\n} while (condition);`
    /// `false` - Ex. `do {\n} while(condition);`
    pub fn do_while_statement_space_after_while_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("doWhileStatement.spaceAfterWhileKeyword", value)
    }

    /// Whether to add spaces around named exports in an export declaration.
    /// * `true` (default) - Ex. `export { SomeExport, OtherExport };`
    /// * `false` - Ex. `export {SomeExport, OtherExport};`
    pub fn export_declaration_space_surrounding_named_exports(&mut self, value: bool) -> &mut Self {
        self.insert("exportDeclaration.spaceSurroundingNamedExports", value)
    }

    /// Whether to add a space after the `for` keyword in a "for" statement.
    /// * `true` (default) - Ex. `for (let i = 0; i < 5; i++)`
    /// * `false` - Ex. `for(let i = 0; i < 5; i++)`
    pub fn for_statement_space_after_for_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("forStatement.spaceAfterForKeyword", value)
    }

    /// Whether to add a space after the semi-colons in a "for" statement.
    /// * `true` (default) - Ex. `for (let i = 0; i < 5; i++)`
    /// * `false` - Ex. `for (let i = 0;i < 5;i++)`
    pub fn for_statement_space_after_semi_colons(&mut self, value: bool) -> &mut Self {
        self.insert("forStatement.spaceAfterSemiColons", value)
    }

    /// Whether to add a space after the `for` keyword in a "for in" statement.
    /// * `true` (default) - Ex. `for (const prop in obj)`
    /// * `false` - Ex. `for(const prop in obj)`
    pub fn for_in_statement_space_after_for_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("forInStatement.spaceAfterForKeyword", value)
    }

    /// Whether to add a space after the `for` keyword in a "for of" statement.
    /// * `true` (default) - Ex. `for (const value of myArray)`
    /// * `false` - Ex. `for(const value of myArray)`
    pub fn for_of_statement_space_after_for_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("forOfStatement.spaceAfterForKeyword", value)
    }

    /// Whether to add a space before the parentheses of a function declaration.
    /// * `true` - Ex. `function myFunction ()`
    /// * `false` (default) - Ex. `function myFunction()`
    pub fn function_declaration_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("functionDeclaration.spaceBeforeParentheses", value)
    }

    /// Whether to add a space before the parentheses of a function expression.
    /// `true` - Ex. `function ()`
    /// `false` (default) - Ex. `function()`
    pub fn function_expression_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("functionExpression.spaceBeforeParentheses", value)
    }

    /// Whether to add a space before the parentheses of a get accessor.
    /// `true` - Ex. `get myProp ()`
    /// `false` (false) - Ex. `get myProp()`
    pub fn get_accessor_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("getAccessor.spaceBeforeParentheses", value)
    }

    /// Whether to add a space after the `if` keyword in an "if" statement.
    /// `true` (default) - Ex. `if (true)`
    /// `false` - Ex. `if(true)`
    pub fn if_statement_space_after_if_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("ifStatement.spaceAfterIfKeyword", value)
    }

    /// Whether to add spaces around named imports in an import declaration.
    /// * `true` (default) - Ex. `import { SomeExport, OtherExport } from "my-module";`
    /// * `false` - Ex. `import {SomeExport, OtherExport} from "my-module";`
    pub fn import_declaration_space_surrounding_named_imports(&mut self, value: bool) -> &mut Self {
        self.insert("importDeclaration.spaceSurroundingNamedImports", value)
    }

    /// Whether to add a space surrounding the expression of a JSX container.
    /// * `true` - Ex. `{ myValue }`
    /// * `false` (default) - Ex. `{myValue}`
    pub fn jsx_expression_container_space_surrounding_expression(&mut self, value: bool) -> &mut Self {
        self.insert("jsxExpressionContainer.spaceSurroundingExpression", value)
    }

    /// Whether to add a space before the parentheses of a method.
    /// `true` - Ex. `myMethod ()`
    /// `false` - Ex. `myMethod()`
    pub fn method_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("method.spaceBeforeParentheses", value)
    }

    /// Whether to add a space before the parentheses of a set accessor.
    /// `true` - Ex. `set myProp (value: string)`
    /// `false` (default) - Ex. `set myProp(value: string)`
    pub fn set_accessor_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("setAccessor.spaceBeforeParentheses", value)
    }

    /// Whether to add a space before the colon of a type annotation.
    /// * `true` - Ex. `function myFunction() : string`
    /// * `false` (default) - Ex. `function myFunction(): string`
    pub fn type_annotation_space_before_colon(&mut self, value: bool) -> &mut Self {
        self.insert("typeAnnotation.spaceBeforeColon", value)
    }

    /// Whether to add a space before the expression in a type assertion.
    /// * `true` (default) - Ex. `<string> myValue`
    /// * `false` - Ex. `<string>myValue`
    pub fn type_assertion_space_before_expression(&mut self, value: bool) -> &mut Self {
        self.insert("typeAssertion.spaceBeforeExpression", value)
    }

    /// Whether to add a space after the `while` keyword in a while statement.
    /// * `true` (default) - Ex. `while (true)`
    /// * `false` - Ex. `while(true)`
    pub fn while_statement_space_after_while_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("whileStatement.spaceAfterWhileKeyword", value)
    }

    /* use parentheses */
    pub fn arrow_function_expression_use_parentheses(&mut self, value: UseParentheses) -> &mut Self {
        self.insert("arrowFunctionExpression.useParentheses", value)
    }

    /* brace position */
    pub fn arrow_function_expression_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("arrowFunctionExpression.bracePosition", value)
    }

    pub fn class_declaration_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("classDeclaration.bracePosition", value)
    }

    pub fn class_expression_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("classExpression.bracePosition", value)
    }

    pub fn constructor_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("constructor.bracePosition", value)
    }

    pub fn do_while_statement_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("doWhileStatement.bracePosition", value)
    }

    pub fn enum_declaration_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("enumDeclaration.bracePosition", value)
    }

    pub fn for_statement_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("forStatement.bracePosition", value)
    }

    pub fn for_in_statement_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("forInStatement.bracePosition", value)
    }

    pub fn for_of_statement_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("forOfStatement.bracePosition", value)
    }

    pub fn get_accessor_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("getAccessor.bracePosition", value)
    }

    pub fn if_statement_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("ifStatement.bracePosition", value)
    }

    pub fn interface_declaration_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("interfaceDeclaration.bracePosition", value)
    }

    pub fn function_declaration_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("functionDeclaration.bracePosition", value)
    }

    pub fn function_expression_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("functionExpression.bracePosition", value)
    }

    pub fn method_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("method.bracePosition", value)
    }

    pub fn module_declaration_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("moduleDeclaration.bracePosition", value)
    }

    pub fn set_accessor_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("setAccessor.bracePosition", value)
    }

    pub fn switch_case_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("switchCase.bracePosition", value)
    }

    pub fn switch_statement_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("switchStatement.bracePosition", value)
    }

    pub fn try_statement_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("tryStatement.bracePosition", value)
    }

    pub fn while_statement_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("whileStatement.bracePosition", value)
    }

    /* force multi-line arguments */
    pub fn call_expression_force_multi_line_arguments(&mut self, value: bool) -> &mut Self {
        self.insert("callExpression.forceMultiLineArguments", value)
    }

    pub fn new_expression_force_multi_line_arguments(&mut self, value: bool) -> &mut Self {
        self.insert("newExpression.forceMultiLineArguments", value)
    }

    /* force multi-line parameters */
    pub fn arrow_function_expression_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("arrowFunctionExpression.forceMultiLineParameters", value)
    }

    pub fn call_signature_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("callSignature.forceMultiLineParameters", value)
    }

    pub fn construct_signature_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("constructSignature.forceMultiLineParameters", value)
    }

    pub fn constructor_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("constructor.forceMultiLineParameters", value)
    }

    pub fn constructor_type_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("constructorType.forceMultiLineParameters", value)
    }

    pub fn function_declaration_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("functionDeclaration.forceMultiLineParameters", value)
    }

    pub fn function_expression_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("functionExpression.forceMultiLineParameters", value)
    }

    pub fn function_type_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("functionType.forceMultiLineParameters", value)
    }

    pub fn get_accessor_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("getAccessor.forceMultiLineParameters", value)
    }

    pub fn method_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("method.forceMultiLineParameters", value)
    }

    pub fn method_signature_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("methodSignature.forceMultiLineParameters", value)
    }

    pub fn set_accessor_force_multi_line_parameters(&mut self, value: bool) -> &mut Self {
        self.insert("setAccessor.forceMultiLineParameters", value)
    }

    /* member spacing */

    pub fn enum_declaration_member_spacing(&mut self, value: MemberSpacing) -> &mut Self {
        self.insert("enumDeclaration.memberSpacing", value)
    }

    /* next control flow position */

    pub fn if_statement_next_control_flow_position(&mut self, value: NextControlFlowPosition) -> &mut Self {
        self.insert("ifStatement.nextControlFlowPosition", value)
    }

    pub fn try_statement_next_control_flow_position(&mut self, value: NextControlFlowPosition) -> &mut Self {
        self.insert("tryStatement.nextControlFlowPosition", value)
    }

    /* operator position */

    pub fn binary_expression_operator_position(&mut self, value: OperatorPosition) -> &mut Self {
        self.insert("binaryExpression.operatorPosition", value)
    }

    pub fn conditional_expression_operator_position(&mut self, value: OperatorPosition) -> &mut Self {
        self.insert("conditionalExpression.operatorPosition", value)
    }

    /* semi-colon */

    pub fn break_statement_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("breakStatement.semiColon", value)
    }

    pub fn call_signature_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("callSignature.semiColon", value)
    }

    pub fn class_property_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("classProperty.semiColon", value)
    }

    pub fn construct_signature_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("constructSignature.semiColon", value)
    }

    pub fn constructor_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("constructor.semiColon", value)
    }

    pub fn continue_statement_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("continueStatement.semiColon", value)
    }

    pub fn debugger_statement_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("debuggerStatement.semiColon", value)
    }

    pub fn do_while_statement_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("doWhileStatement.semiColon", value)
    }

    pub fn export_all_declaration_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("exportAllDeclaration.semiColon", value)
    }

    pub fn export_assignment_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("exportAssignment.semiColon", value)
    }

    pub fn export_default_expression_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("exportDefaultExpression.semiColon", value)
    }

    pub fn export_named_declaration_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("exportNamedDeclaration.semiColon", value)
    }

    pub fn expression_statement_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("expressionStatement.semiColon", value)
    }

    pub fn function_declaration_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("functionDeclaration.semiColon", value)
    }

    pub fn get_accessor_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("getAccessor.semiColon", value)
    }

    pub fn import_declaration_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("importDeclaration.semiColon", value)
    }

    pub fn import_equals_declaration_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("importEqualsDeclaration.semiColon", value)
    }

    pub fn index_signature_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("indexSignature.semiColon", value)
    }

    pub fn mapped_type_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("mappedType.semiColon", value)
    }

    pub fn method_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("method.semiColon", value)
    }

    pub fn method_signature_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("methodSignature.semiColon", value)
    }

    pub fn module_declaration_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("moduleDeclaration.semiColon", value)
    }

    pub fn namespace_export_declaration_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("namespaceExportDeclaration.semiColon", value)
    }

    pub fn property_signature_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("propertySignature.semiColon", value)
    }

    pub fn return_statement_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("returnStatement.semiColon", value)
    }

    pub fn set_accessor_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("setAccessor.semiColon", value)
    }

    pub fn throw_statement_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("throwStatement.semiColon", value)
    }

    pub fn type_alias_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("typeAlias.semiColon", value)
    }

    pub fn variable_statement_semi_colon(&mut self, value: bool) -> &mut Self {
        self.insert("variableStatement.semiColon", value)
    }

    /* single body position */
    pub fn if_statement_single_body_position(&mut self, value: SingleBodyPosition) -> &mut Self {
        self.insert("ifStatement.singleBodyPosition", value)
    }

    pub fn for_statement_single_body_position(&mut self, value: SingleBodyPosition) -> &mut Self {
        self.insert("forStatement.singleBodyPosition", value)
    }

    pub fn for_in_statement_single_body_position(&mut self, value: SingleBodyPosition) -> &mut Self {
        self.insert("forInStatement.singleBodyPosition", value)
    }

    pub fn for_of_statement_single_body_position(&mut self, value: SingleBodyPosition) -> &mut Self {
        self.insert("forOfStatement.singleBodyPosition", value)
    }

    pub fn while_statement_single_body_position(&mut self, value: SingleBodyPosition) -> &mut Self {
        self.insert("whileStatement.singleBodyPosition", value)
    }

    /* trailing commas */

    pub fn array_expression_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("arrayExpression.trailingCommas", value)
    }

    pub fn array_pattern_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("arrayPattern.trailingCommas", value)
    }

    pub fn enum_declaration_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("enumDeclaration.trailingCommas", value)
    }

    pub fn object_expression_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("objectExpression.trailingCommas", value)
    }

    pub fn tuple_type_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("tupleType.trailingCommas", value)
    }

    /* use braces */

    pub fn if_statement_use_braces(&mut self, value: UseBraces) -> &mut Self {
        self.insert("ifStatement.useBraces", value)
    }

    pub fn for_statement_use_braces(&mut self, value: UseBraces) -> &mut Self {
        self.insert("forStatement.useBraces", value)
    }

    pub fn for_in_statement_use_braces(&mut self, value: UseBraces) -> &mut Self {
        self.insert("forInStatement.useBraces", value)
    }

    pub fn for_of_statement_use_braces(&mut self, value: UseBraces) -> &mut Self {
        self.insert("forOfStatement.useBraces", value)
    }

    pub fn while_statement_use_braces(&mut self, value: UseBraces) -> &mut Self {
        self.insert("whileStatement.useBraces", value)
    }

    #[cfg(test)]
    pub(super) fn get_inner_config(&self) -> HashMap<String, String> {
        self.config.clone()
    }

    fn insert<T>(&mut self, name: &str, value: T) -> &mut Self where T : std::string::ToString {
        self.config.insert(String::from(name), value.to_string());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseConfigurationError(String);

impl std::fmt::Display for ParseConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format!("Found invalid value '{}'.", self.0).fmt(f)
    }
}

macro_rules! generate_str_to_from {
    ($enum_name:ident, $([$member_name:ident, $string_value:expr]),* ) => {
        impl std::str::FromStr for $enum_name {
            type Err = ParseConfigurationError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($string_value => Ok($enum_name::$member_name)),*,
                    _ => Err(ParseConfigurationError(String::from(s))),
                }
            }
        }

        impl std::string::ToString for $enum_name {
            fn to_string(&self) -> String {
                match self {
                    $($enum_name::$member_name => String::from($string_value)),*,
                }
            }
        }
    };
}

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

generate_str_to_from![
    NewLineKind,
    [Auto, "auto"],
    [Unix, "\n"],
    [Windows, "\r\n"]
];

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

generate_str_to_from![
    TrailingCommas,
    [Always, "always"],
    [Never, "never"],
    [OnlyMultiLine, "onlyMultiLine"]
];

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

generate_str_to_from![
    BracePosition,
    [Maintain, "maintain"],
    [SameLine, "sameLine"],
    [NextLine, "nextLine"],
    [NextLineIfHanging, "nextLineIfHanging"]
];

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

generate_str_to_from![
    MemberSpacing,
    [Maintain, "maintain"],
    [BlankLine, "blankline"],
    [NewLine, "newline"]
];

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

generate_str_to_from![
    NextControlFlowPosition,
    [Maintain, "maintain"],
    [SameLine, "sameLine"],
    [NextLine, "nextLine"]
];

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

generate_str_to_from![
    OperatorPosition,
    [Maintain, "maintain"],
    [SameLine, "sameLine"],
    [NextLine, "nextLine"]
];

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

generate_str_to_from![
    SingleBodyPosition,
    [Maintain, "maintain"],
    [SameLine, "sameLine"],
    [NextLine, "nextLine"]
];

/// If braces should be used or not in certain scenarios.
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

generate_str_to_from![
    UseBraces,
    [Maintain, "maintain"],
    [WhenNotSingleLine, "whenNotSingleLine"],
    [Always, "always"],
    [PreferNone, "preferNone"]
];

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

generate_str_to_from![
    UseParentheses,
    [Maintain, "maintain"],
    [Force, "force"],
    [PreferNone, "preferNone"]
];

/// Represents a problem within the configuration.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationDiagnostic {
    /// The property name the problem occurred on.
    pub property_name: String,
    /// The diagnostic message that should be displayed to the user
    pub message: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveConfigurationResult {
    /// The configuration diagnostics.
    pub diagnostics: Vec<ConfigurationDiagnostic>,

    /// The configuration derived from the unresolved configuration
    /// that can be used to format a file.
    pub config: ResolvedTypeScriptConfiguration,
}

/// Resolves configuration from a collection of key value strings.
///
/// Note: You most likely want to use `TypeScriptConfiguration` instead.
pub fn resolve_config(config: &HashMap<String, String>) -> ResolveConfigurationResult {
    let mut diagnostics = Vec::new();
    let mut config = config.clone();

    let semi_colons = get_value(&mut config, "semiColons", true, &mut diagnostics);
    let force_multi_line_arguments = get_value(&mut config, "forceMultiLineArguments", false, &mut diagnostics);
    let force_multi_line_parameters = get_value(&mut config, "forceMultiLineParameters", false, &mut diagnostics);
    let brace_position = get_value(&mut config, "bracePosition", BracePosition::NextLineIfHanging, &mut diagnostics);
    let next_control_flow_position = get_value(&mut config, "nextControlFlowPosition", NextControlFlowPosition::NextLine, &mut diagnostics);
    let operator_position = get_value(&mut config, "operatorPosition", OperatorPosition::NextLine, &mut diagnostics);
    let single_body_position = get_value(&mut config, "singleBodyPosition", SingleBodyPosition::Maintain, &mut diagnostics);
    let trailing_commas = get_value(&mut config, "trailingCommas", TrailingCommas::Never, &mut diagnostics);
    let use_braces = get_value(&mut config, "useBraces", UseBraces::WhenNotSingleLine, &mut diagnostics);

    let resolved_config = ResolvedTypeScriptConfiguration {
        line_width: get_value(&mut config, "lineWidth", 120, &mut diagnostics),
        use_tabs: get_value(&mut config, "useTabs", false, &mut diagnostics),
        indent_width: get_value(&mut config, "indentWidth", 4, &mut diagnostics),
        single_quotes: get_value(&mut config, "singleQuotes", false, &mut diagnostics),
        new_line_kind: get_value(&mut config, "newLineKind", NewLineKind::Auto, &mut diagnostics),
        /* use parentheses */
        arrow_function_expression_use_parentheses: get_value(&mut config, "arrowFunctionExpression.useParentheses", UseParentheses::Maintain, &mut diagnostics),
        /* brace position */
        arrow_function_expression_brace_position: get_value(&mut config, "arrowFunctionExpression.bracePosition", brace_position, &mut diagnostics),
        class_declaration_brace_position: get_value(&mut config, "classDeclaration.bracePosition", brace_position, &mut diagnostics),
        class_expression_brace_position: get_value(&mut config, "classExpression.bracePosition", brace_position, &mut diagnostics),
        constructor_brace_position: get_value(&mut config, "constructor.bracePosition", brace_position, &mut diagnostics),
        do_while_statement_brace_position: get_value(&mut config, "doWhileStatement.bracePosition", brace_position, &mut diagnostics),
        enum_declaration_brace_position: get_value(&mut config, "enumDeclaration.bracePosition", brace_position, &mut diagnostics),
        for_statement_brace_position: get_value(&mut config, "forStatement.bracePosition", brace_position, &mut diagnostics),
        for_in_statement_brace_position: get_value(&mut config, "forInStatement.bracePosition", brace_position, &mut diagnostics),
        for_of_statement_brace_position: get_value(&mut config, "forOfStatement.bracePosition", brace_position, &mut diagnostics),
        get_accessor_brace_position: get_value(&mut config, "getAccessor.bracePosition", brace_position, &mut diagnostics),
        if_statement_brace_position: get_value(&mut config, "ifStatement.bracePosition", brace_position, &mut diagnostics),
        interface_declaration_brace_position: get_value(&mut config, "interfaceDeclaration.bracePosition", brace_position, &mut diagnostics),
        function_declaration_brace_position: get_value(&mut config, "functionDeclaration.bracePosition", brace_position, &mut diagnostics),
        function_expression_brace_position: get_value(&mut config, "functionExpression.bracePosition", brace_position, &mut diagnostics),
        method_brace_position: get_value(&mut config, "method.bracePosition", brace_position, &mut diagnostics),
        module_declaration_brace_position: get_value(&mut config, "moduleDeclaration.bracePosition", brace_position, &mut diagnostics),
        set_accessor_brace_position: get_value(&mut config, "setAccessor.bracePosition", brace_position, &mut diagnostics),
        switch_case_brace_position: get_value(&mut config, "switchCase.bracePosition", brace_position, &mut diagnostics),
        switch_statement_brace_position: get_value(&mut config, "switchStatement.bracePosition", brace_position, &mut diagnostics),
        try_statement_brace_position: get_value(&mut config, "tryStatement.bracePosition", brace_position, &mut diagnostics),
        while_statement_brace_position: get_value(&mut config, "whileStatement.bracePosition", brace_position, &mut diagnostics),
        /* force multi-line arguments */
        call_expression_force_multi_line_arguments: get_value(&mut config, "callExpression.forceMultiLineArguments", force_multi_line_arguments, &mut diagnostics),
        new_expression_force_multi_line_arguments: get_value(&mut config, "newExpression.forceMultiLineArguments", force_multi_line_arguments, &mut diagnostics),
        /* force multi-line parameters */
        arrow_function_expression_force_multi_line_parameters: get_value(&mut config, "arrowFunctionExpression.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        call_signature_force_multi_line_parameters: get_value(&mut config, "callSignature.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        construct_signature_force_multi_line_parameters: get_value(&mut config, "constructSignature.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        constructor_force_multi_line_parameters: get_value(&mut config, "constructor.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        constructor_type_force_multi_line_parameters: get_value(&mut config, "constructorType.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        function_declaration_force_multi_line_parameters: get_value(&mut config, "functionDeclaration.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        function_expression_force_multi_line_parameters: get_value(&mut config, "functionExpression.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        function_type_force_multi_line_parameters: get_value(&mut config, "functionType.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        get_accessor_force_multi_line_parameters: get_value(&mut config, "getAccessor.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        method_force_multi_line_parameters: get_value(&mut config, "method.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        method_signature_force_multi_line_parameters: get_value(&mut config, "methodSignature.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        set_accessor_force_multi_line_parameters: get_value(&mut config, "setAccessor.forceMultiLineParameters", force_multi_line_parameters, &mut diagnostics),
        /* member spacing */
        enum_declaration_member_spacing: get_value(&mut config, "enumDeclaration.memberSpacing", MemberSpacing::Maintain, &mut diagnostics),
        /* next control flow position */
        if_statement_next_control_flow_position: get_value(&mut config, "ifStatement.nextControlFlowPosition", next_control_flow_position, &mut diagnostics),
        try_statement_next_control_flow_position: get_value(&mut config, "tryStatement.nextControlFlowPosition", next_control_flow_position, &mut diagnostics),
        /* operator position */
        binary_expression_operator_position: get_value(&mut config, "binaryExpression.operatorPosition", operator_position, &mut diagnostics),
        conditional_expression_operator_position: get_value(&mut config, "conditionalExpression.operatorPosition", operator_position, &mut diagnostics),
        /* semi-colon */
        break_statement_semi_colon: get_value(&mut config, "breakStatement.semiColon", semi_colons, &mut diagnostics),
        call_signature_semi_colon: get_value(&mut config, "callSignature.semiColon", semi_colons, &mut diagnostics),
        class_property_semi_colon: get_value(&mut config, "classProperty.semiColon", semi_colons, &mut diagnostics),
        construct_signature_semi_colon: get_value(&mut config, "constructSignature.semiColon", semi_colons, &mut diagnostics),
        constructor_semi_colon: get_value(&mut config, "constructor.semiColon", semi_colons, &mut diagnostics),
        continue_statement_semi_colon: get_value(&mut config, "continueStatement.semiColon", semi_colons, &mut diagnostics),
        debugger_statement_semi_colon: get_value(&mut config, "debuggerStatement.semiColon", semi_colons, &mut diagnostics),
        do_while_statement_semi_colon: get_value(&mut config, "doWhileStatement.semiColon", semi_colons, &mut diagnostics),
        export_all_declaration_semi_colon: get_value(&mut config, "exportAllDeclaration.semiColon", semi_colons, &mut diagnostics),
        export_assignment_semi_colon: get_value(&mut config, "exportAssignment.semiColon", semi_colons, &mut diagnostics),
        export_default_expression_semi_colon: get_value(&mut config, "exportDefaultExpression.semiColon", semi_colons, &mut diagnostics),
        export_named_declaration_semi_colon: get_value(&mut config, "exportNamedDeclaration.semiColon", semi_colons, &mut diagnostics),
        expression_statement_semi_colon: get_value(&mut config, "expressionStatement.semiColon", semi_colons, &mut diagnostics),
        function_declaration_semi_colon: get_value(&mut config, "functionDeclaration.semiColon", semi_colons, &mut diagnostics),
        get_accessor_semi_colon: get_value(&mut config, "getAccessor.semiColon", semi_colons, &mut diagnostics),
        import_declaration_semi_colon: get_value(&mut config, "importDeclaration.semiColon", semi_colons, &mut diagnostics),
        import_equals_declaration_semi_colon: get_value(&mut config, "importEqualsDeclaration.semiColon", semi_colons, &mut diagnostics),
        index_signature_semi_colon: get_value(&mut config, "indexSignature.semiColon", semi_colons, &mut diagnostics),
        mapped_type_semi_colon: get_value(&mut config, "mappedType.semiColon", semi_colons, &mut diagnostics),
        method_semi_colon: get_value(&mut config, "method.semiColon", semi_colons, &mut diagnostics),
        method_signature_semi_colon: get_value(&mut config, "methodSignature.semiColon", semi_colons, &mut diagnostics),
        module_declaration_semi_colon: get_value(&mut config, "moduleDeclaration.semiColon", semi_colons, &mut diagnostics),
        namespace_export_declaration_semi_colon: get_value(&mut config, "namespaceExportDeclaration.semiColon", semi_colons, &mut diagnostics),
        property_signature_semi_colon: get_value(&mut config, "propertySignature.semiColon", semi_colons, &mut diagnostics),
        return_statement_semi_colon: get_value(&mut config, "returnStatement.semiColon", semi_colons, &mut diagnostics),
        set_accessor_semi_colon: get_value(&mut config, "setAccessor.semiColon", semi_colons, &mut diagnostics),
        throw_statement_semi_colon: get_value(&mut config, "throwStatement.semiColon", semi_colons, &mut diagnostics),
        type_alias_semi_colon: get_value(&mut config, "typeAlias.semiColon", semi_colons, &mut diagnostics),
        variable_statement_semi_colon: get_value(&mut config, "variableStatement.semiColon", semi_colons, &mut diagnostics),
        /* single body position */
        if_statement_single_body_position: get_value(&mut config, "ifStatement.singleBodyPosition", single_body_position, &mut diagnostics),
        for_statement_single_body_position: get_value(&mut config, "forStatement.singleBodyPosition", single_body_position, &mut diagnostics),
        for_in_statement_single_body_position: get_value(&mut config, "forInStatement.singleBodyPosition", single_body_position, &mut diagnostics),
        for_of_statement_single_body_position: get_value(&mut config, "forOfStatement.singleBodyPosition", single_body_position, &mut diagnostics),
        while_statement_single_body_position: get_value(&mut config, "whileStatement.singleBodyPosition", single_body_position, &mut diagnostics),
        /* trailing commas */
        array_expression_trailing_commas: get_value(&mut config, "arrayExpression.trailingCommas", trailing_commas, &mut diagnostics),
        array_pattern_trailing_commas: get_value(&mut config, "arrayPattern.trailingCommas", trailing_commas, &mut diagnostics),
        enum_declaration_trailing_commas: get_value(&mut config, "enumDeclaration.trailingCommas", trailing_commas, &mut diagnostics),
        object_expression_trailing_commas: get_value(&mut config, "objectExpression.trailingCommas", trailing_commas, &mut diagnostics),
        tuple_type_trailing_commas: get_value(&mut config, "tupleType.trailingCommas", trailing_commas, &mut diagnostics),
        /* use braces */
        if_statement_use_braces: get_value(&mut config, "ifStatement.useBraces", use_braces, &mut diagnostics),
        for_statement_use_braces: get_value(&mut config, "forStatement.useBraces", use_braces, &mut diagnostics),
        for_in_statement_use_braces: get_value(&mut config, "forInStatement.useBraces", use_braces, &mut diagnostics),
        for_of_statement_use_braces: get_value(&mut config, "forOfStatement.useBraces", use_braces, &mut diagnostics),
        while_statement_use_braces: get_value(&mut config, "whileStatement.useBraces", use_braces, &mut diagnostics),
        /* space settings */
        binary_expression_space_surrounding_bitwise_and_arithmetic_operator: get_value(&mut config, "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator", true, &mut diagnostics),
        construct_signature_space_after_new_keyword: get_value(&mut config, "constructSignature.spaceAfterNewKeyword", false, &mut diagnostics),
        constructor_space_before_parentheses: get_value(&mut config, "constructor.spaceBeforeParentheses", false, &mut diagnostics),
        constructor_type_space_after_new_keyword: get_value(&mut config, "constructorType.spaceAfterNewKeyword", false, &mut diagnostics),
        do_while_statement_space_after_while_keyword: get_value(&mut config, "doWhileStatement.spaceAfterWhileKeyword", true, &mut diagnostics),
        export_declaration_space_surrounding_named_exports: get_value(&mut config, "exportDeclaration.spaceSurroundingNamedExports", true, &mut diagnostics),
        for_statement_space_after_for_keyword: get_value(&mut config, "forStatement.spaceAfterForKeyword", true, &mut diagnostics),
        for_statement_space_after_semi_colons: get_value(&mut config, "forStatement.spaceAfterSemiColons", true, &mut diagnostics),
        for_in_statement_space_after_for_keyword: get_value(&mut config, "forInStatement.spaceAfterForKeyword", true, &mut diagnostics),
        for_of_statement_space_after_for_keyword: get_value(&mut config, "forOfStatement.spaceAfterForKeyword", true, &mut diagnostics),
        function_declaration_space_before_parentheses: get_value(&mut config, "functionDeclaration.spaceBeforeParentheses", false, &mut diagnostics),
        function_expression_space_before_parentheses: get_value(&mut config, "functionExpression.spaceBeforeParentheses", false, &mut diagnostics),
        get_accessor_space_before_parentheses: get_value(&mut config, "getAccessor.spaceBeforeParentheses", false, &mut diagnostics),
        if_statement_space_after_if_keyword: get_value(&mut config, "ifStatement.spaceAfterIfKeyword", true, &mut diagnostics),
        import_declaration_space_surrounding_named_imports: get_value(&mut config, "importDeclaration.spaceSurroundingNamedImports", true, &mut diagnostics),
        jsx_expression_container_space_surrounding_expression: get_value(&mut config, "jsxExpressionContainer.spaceSurroundingExpression", false, &mut diagnostics),
        method_space_before_parentheses: get_value(&mut config, "method.spaceBeforeParentheses", false, &mut diagnostics),
        set_accessor_space_before_parentheses: get_value(&mut config, "setAccessor.spaceBeforeParentheses", false, &mut diagnostics),
        type_annotation_space_before_colon: get_value(&mut config, "typeAnnotation.spaceBeforeColon", false, &mut diagnostics),
        type_assertion_space_before_expression: get_value(&mut config, "typeAssertion.spaceBeforeExpression", true, &mut diagnostics),
        while_statement_space_after_while_keyword: get_value(&mut config, "whileStatement.spaceAfterWhileKeyword", true, &mut diagnostics),
    };

    for (key, _) in config.iter() {
        diagnostics.push(ConfigurationDiagnostic {
            property_name: String::from(key),
            message: format!("Unexpected property in configuration: {}", key),
        });
    }

    ResolveConfigurationResult {
        config: resolved_config,
        diagnostics,
    }
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTypeScriptConfiguration {
    pub indent_width: u8,
    pub line_width: u32,
    pub use_tabs: bool,
    pub single_quotes: bool,
    pub new_line_kind: NewLineKind,
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

    #[serde(rename = "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator")]
    pub binary_expression_space_surrounding_bitwise_and_arithmetic_operator: bool,
    #[serde(rename = "constructSignature.spaceAfterNewKeyword")]
    pub construct_signature_space_after_new_keyword: bool,
    #[serde(rename = "constructor.spaceBeforeParentheses")]
    pub constructor_space_before_parentheses: bool,
    #[serde(rename = "constructorType.spaceAfterNewKeyword")]
    pub constructor_type_space_after_new_keyword: bool,
    #[serde(rename = "doWhileStatement.spaceAfterWhileKeyword")]
    pub do_while_statement_space_after_while_keyword: bool,
    #[serde(rename = "exportDeclarationSpace.surroundingNamedExports")]
    pub export_declaration_space_surrounding_named_exports: bool,
    #[serde(rename = "forStatement.spaceAfterForKeyword")]
    pub for_statement_space_after_for_keyword: bool,
    #[serde(rename = "forStatement.spaceAfterSemiColons")]
    pub for_statement_space_after_semi_colons: bool,
    #[serde(rename = "forInStatement.spaceAfterForKeyword")]
    pub for_in_statement_space_after_for_keyword: bool,
    #[serde(rename = "forOfStatement.spaceAfterForKeyword")]
    pub for_of_statement_space_after_for_keyword: bool,
    #[serde(rename = "functionDeclaration.spaceBeforeParentheses")]
    pub function_declaration_space_before_parentheses: bool,
    #[serde(rename = "functionExpression.spaceBeforeParentheses")]
    pub function_expression_space_before_parentheses: bool,
    #[serde(rename = "getAccessor.spaceBeforeParentheses")]
    pub get_accessor_space_before_parentheses: bool,
    #[serde(rename = "ifStatement.spaceAfterIfKeyword")]
    pub if_statement_space_after_if_keyword: bool,
    #[serde(rename = "importDeclaration.spaceSurroundingNamedImports")]
    pub import_declaration_space_surrounding_named_imports: bool,
    #[serde(rename = "jsxExpressionContainer.spaceSurroundingExpression")]
    pub jsx_expression_container_space_surrounding_expression: bool,
    #[serde(rename = "method.spaceBeforeParentheses")]
    pub method_space_before_parentheses: bool,
    #[serde(rename = "setAccessor.spaceBeforeParentheses")]
    pub set_accessor_space_before_parentheses: bool,
    #[serde(rename = "typeAnnotation.spaceBeforeColon")]
    pub type_annotation_space_before_colon: bool,
    #[serde(rename = "typeAssertion.spaceBeforeExpression")]
    pub type_assertion_space_before_expression: bool,
    #[serde(rename = "whileStatement.spaceAfterWhileKeyword")]
    pub while_statement_space_after_while_keyword: bool,
}

// todo: more tests, but this is currently tested by the javascript code in dprint-plugin-typescript
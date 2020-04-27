use std::collections::HashMap;
use dprint_core::configuration::*;
use super::resolve_config::resolve_config;
use super::types::*;

/// TypeScript formatting configuration builder.
///
/// # Example
///
/// ```
/// use dprint_plugin_typescript::configuration::*;
///
/// let config = ConfigurationBuilder::new()
///     .line_width(80)
///     .prefer_hanging(true)
///     .prefer_single_line(false)
///     .quote_style(QuoteStyle::PreferSingle)
///     .next_control_flow_position(NextControlFlowPosition::SameLine)
///     .build();
/// ```
pub struct ConfigurationBuilder {
    pub(super) config: HashMap<String, String>,
    global_config: Option<GlobalConfiguration>,
}

impl ConfigurationBuilder {
    /// Constructs a new configuration builder.
    pub fn new() -> ConfigurationBuilder {
        ConfigurationBuilder {
            config: HashMap::new(),
            global_config: None,
        }
    }

    /// Gets the final configuration that can be used to format a file.
    pub fn build(&self) -> Configuration {
        if let Some(global_config) = &self.global_config {
            resolve_config(self.config.clone(), global_config).config
        } else {
            let global_config = resolve_global_config(&HashMap::new()).config;
            resolve_config(self.config.clone(), &global_config).config
        }
    }

    /// Set the global configuration.
    pub fn global_config(&mut self, global_config: GlobalConfiguration) -> &mut Self {
        self.global_config = Some(global_config);
        self
    }

    /// Helper method to set the configuration to what's used for Deno.
    pub fn deno(&mut self) -> &mut Self {
        self.line_width(80)
            .indent_width(2)
            .next_control_flow_position(NextControlFlowPosition::SameLine)
            .binary_expression_operator_position(OperatorPosition::SameLine)
            .brace_position(BracePosition::SameLine)
            .comment_line_force_space_after_slashes(false)
            .construct_signature_space_after_new_keyword(true)
            .constructor_type_space_after_new_keyword(true)
            .arrow_function_use_parentheses(UseParentheses::Force)
            .new_line_kind(NewLineKind::LineFeed)
            .function_expression_space_after_function_keyword(true)
            .tagged_template_space_before_literal(false)
            .conditional_expression_prefer_single_line(true)
    }

    /// The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.
    ///
    /// Default: `120`
    pub fn line_width(&mut self, value: u32) -> &mut Self {
        self.insert("lineWidth", value)
    }

    /// Whether to use tabs (true) or spaces (false).
    ///
    /// Default: `false`
    pub fn use_tabs(&mut self, value: bool) -> &mut Self {
        self.insert("useTabs", value)
    }

    /// The number of columns for an indent.
    ///
    /// Default: `4`
    pub fn indent_width(&mut self, value: u8) -> &mut Self {
        self.insert("indentWidth", value)
    }

    /// The kind of newline to use.
    ///
    /// Default: `NewLineKind::LineFeed`
    pub fn new_line_kind(&mut self, value: NewLineKind) -> &mut Self {
        self.insert("newLineKind", value)
    }

    /// The quote style to use.
    ///
    /// Default: `QuoteStyle::PreferDouble`
    pub fn quote_style(&mut self, value: QuoteStyle) -> &mut Self {
        self.insert("quoteStyle", value)
    }

    /// Whether statements should end in a semi-colon.
    ///
    /// Default: `SemiColons::Prefer`
    pub fn semi_colons(&mut self, value: SemiColons) -> &mut Self {
        self.insert("semiColons", value)
    }

    /// Set to prefer hanging indentation when exceeding the line width.
    ///
    /// Default: `false`
    pub fn prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("preferHanging", value)
    }

    /// Where to place the opening brace.
    ///
    /// Default: `BracePosition::NextLineIfHanging`
    pub fn brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("bracePosition", value)
    }

    /// Where to place the next control flow within a control flow statement.
    ///
    /// Default: `NextControlFlowPosition::NextLine`
    pub fn next_control_flow_position(&mut self, value: NextControlFlowPosition) -> &mut Self {
        self.insert("nextControlFlowPosition", value)
    }

    /// Where to place the operator for expressions that span multiple lines.
    ///
    /// Default: `OperatorPosition::NextLine`
    pub fn operator_position(&mut self, value: OperatorPosition) -> &mut Self {
        self.insert("operatorPosition", value)
    }

    /// Where to place the expression of a statement that could possibly be on one line (ex. `if (true) console.log(5);`).
    ///
    /// Default: SingleBodyPosition::Maintain
    pub fn single_body_position(&mut self, value: SingleBodyPosition) -> &mut Self {
        self.insert("singleBodyPosition", value)
    }

    /// If trailing commas should be used.
    ///
    /// Default: `TrailingCommas::OnlyMultiLine`
    pub fn trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("trailingCommas", value)
    }

    /// If braces should be used or not.
    ///
    /// Default: `UseBraces::WhenNotSingleLine`
    pub fn use_braces(&mut self, value: UseBraces) -> &mut Self {
        self.insert("useBraces", value)
    }

    /// If code should revert back from being on multiple lines to
    /// being on a single line when able.
    ///
    /// Default: `false`
    pub fn prefer_single_line(&mut self, value: bool) -> &mut Self {
        self.insert("preferSingleLine", value)
    }

    /* space settings */

    /// Whether to surround bitwise and arithmetic operators in a binary expression with spaces.
    ///
    /// * `true` (default) - Ex. `1 + 2`
    /// * `false` - Ex. `1+2`
    pub fn binary_expression_space_surrounding_bitwise_and_arithmetic_operator(&mut self, value: bool) -> &mut Self {
        self.insert("binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator", value)
    }

    /// Forces a space after the double slash in a comment line.
    ///
    /// `true` (default) - Ex. `//test` -> `// test`
    /// `false` - Ex. `//test` -> `//test`
    pub fn comment_line_force_space_after_slashes(&mut self, value: bool) -> &mut Self {
        self.insert("commentLine.forceSpaceAfterSlashes", value)
    }

    /// Whether to add a space after the `new` keyword in a construct signature.
    ///
    /// `true` - Ex. `new (): MyClass;`
    /// `false` (default) - Ex. `new(): MyClass;`
    pub fn construct_signature_space_after_new_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("constructSignature.spaceAfterNewKeyword", value)
    }

    /// Whether to add a space before the parentheses of a constructor.
    ///
    /// `true` - Ex. `constructor ()`
    /// `false` (false) - Ex. `constructor()`
    pub fn constructor_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("constructor.spaceBeforeParentheses", value)
    }

    /// Whether to add a space after the `new` keyword in a constructor type.
    ///
    /// `true` - Ex. `type MyClassCtor = new () => MyClass;`
    /// `false` (default) - Ex. `type MyClassCtor = new() => MyClass;`
    pub fn constructor_type_space_after_new_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("constructorType.spaceAfterNewKeyword", value)
    }

    /// Whether to add a space after the `while` keyword in a do while statement.
    ///
    /// `true` (true) - Ex. `do {\n} while (condition);`
    /// `false` - Ex. `do {\n} while(condition);`
    pub fn do_while_statement_space_after_while_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("doWhileStatement.spaceAfterWhileKeyword", value)
    }

    /// Whether to add spaces around named exports in an export declaration.
    ///
    /// * `true` (default) - Ex. `export { SomeExport, OtherExport };`
    /// * `false` - Ex. `export {SomeExport, OtherExport};`
    pub fn export_declaration_space_surrounding_named_exports(&mut self, value: bool) -> &mut Self {
        self.insert("exportDeclaration.spaceSurroundingNamedExports", value)
    }

    /// Whether to add a space after the `for` keyword in a "for" statement.
    ///
    /// * `true` (default) - Ex. `for (let i = 0; i < 5; i++)`
    /// * `false` - Ex. `for(let i = 0; i < 5; i++)`
    pub fn for_statement_space_after_for_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("forStatement.spaceAfterForKeyword", value)
    }

    /// Whether to add a space after the semi-colons in a "for" statement.
    ///
    /// * `true` (default) - Ex. `for (let i = 0; i < 5; i++)`
    /// * `false` - Ex. `for (let i = 0;i < 5;i++)`
    pub fn for_statement_space_after_semi_colons(&mut self, value: bool) -> &mut Self {
        self.insert("forStatement.spaceAfterSemiColons", value)
    }

    /// Whether to add a space after the `for` keyword in a "for in" statement.
    ///
    /// * `true` (default) - Ex. `for (const prop in obj)`
    /// * `false` - Ex. `for(const prop in obj)`
    pub fn for_in_statement_space_after_for_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("forInStatement.spaceAfterForKeyword", value)
    }

    /// Whether to add a space after the `for` keyword in a "for of" statement.
    ///
    /// * `true` (default) - Ex. `for (const value of myArray)`
    /// * `false` - Ex. `for(const value of myArray)`
    pub fn for_of_statement_space_after_for_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("forOfStatement.spaceAfterForKeyword", value)
    }

    /// Whether to add a space before the parentheses of a function declaration.
    ///
    /// * `true` - Ex. `function myFunction ()`
    /// * `false` (default) - Ex. `function myFunction()`
    pub fn function_declaration_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("functionDeclaration.spaceBeforeParentheses", value)
    }

    /// Whether to add a space before the parentheses of a function expression.
    ///
    /// `true` - Ex. `function<T> ()`
    /// `false` (default) - Ex. `function<T> ()`
    pub fn function_expression_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("functionExpression.spaceBeforeParentheses", value)
    }

    /// Whether to add a space after the function keyword of a function expression.
    ///
    /// `true` - Ex. `function <T>()`.
    /// `false` (default) - Ex. `function<T>()`
    pub fn function_expression_space_after_function_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("functionExpression.spaceAfterFunctionKeyword", value)
    }

    /// Whether to add a space before the parentheses of a get accessor.
    ///
    /// `true` - Ex. `get myProp ()`
    /// `false` (false) - Ex. `get myProp()`
    pub fn get_accessor_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("getAccessor.spaceBeforeParentheses", value)
    }

    /// Whether to add a space after the `if` keyword in an "if" statement.
    ///
    /// `true` (default) - Ex. `if (true)`
    /// `false` - Ex. `if(true)`
    pub fn if_statement_space_after_if_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("ifStatement.spaceAfterIfKeyword", value)
    }

    /// Whether to add spaces around named imports in an import declaration.
    ///
    /// * `true` (default) - Ex. `import { SomeExport, OtherExport } from "my-module";`
    /// * `false` - Ex. `import {SomeExport, OtherExport} from "my-module";`
    pub fn import_declaration_space_surrounding_named_imports(&mut self, value: bool) -> &mut Self {
        self.insert("importDeclaration.spaceSurroundingNamedImports", value)
    }

    /// Whether to add a space surrounding the expression of a JSX container.
    ///
    /// * `true` - Ex. `{ myValue }`
    /// * `false` (default) - Ex. `{myValue}`
    pub fn jsx_expression_container_space_surrounding_expression(&mut self, value: bool) -> &mut Self {
        self.insert("jsxExpressionContainer.spaceSurroundingExpression", value)
    }

    /// Whether to add a space before the parentheses of a method.
    ///
    /// `true` - Ex. `myMethod ()`
    /// `false` - Ex. `myMethod()`
    pub fn method_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("method.spaceBeforeParentheses", value)
    }

    /// Whether to add a space before the parentheses of a set accessor.
    ///
    /// `true` - Ex. `set myProp (value: string)`
    /// `false` (default) - Ex. `set myProp(value: string)`
    pub fn set_accessor_space_before_parentheses(&mut self, value: bool) -> &mut Self {
        self.insert("setAccessor.spaceBeforeParentheses", value)
    }

    /// Whether to add a space before the literal in a tagged template.
    ///
    /// `true` (default) - Ex. `html \`<element />\``
    /// `false` - Ex. `html\`<element />\``
    pub fn tagged_template_space_before_literal(&mut self, value: bool) -> &mut Self {
        self.insert("taggedTemplate.spaceBeforeLiteral", value)
    }

    /// Whether to add a space before the colon of a type annotation.
    ///
    /// * `true` - Ex. `function myFunction() : string`
    /// * `false` (default) - Ex. `function myFunction(): string`
    pub fn type_annotation_space_before_colon(&mut self, value: bool) -> &mut Self {
        self.insert("typeAnnotation.spaceBeforeColon", value)
    }

    /// Whether to add a space before the expression in a type assertion.
    ///
    /// * `true` (default) - Ex. `<string> myValue`
    /// * `false` - Ex. `<string>myValue`
    pub fn type_assertion_space_before_expression(&mut self, value: bool) -> &mut Self {
        self.insert("typeAssertion.spaceBeforeExpression", value)
    }

    /// Whether to add a space after the `while` keyword in a while statement.
    ///
    /// * `true` (default) - Ex. `while (true)`
    /// * `false` - Ex. `while(true)`
    pub fn while_statement_space_after_while_keyword(&mut self, value: bool) -> &mut Self {
        self.insert("whileStatement.spaceAfterWhileKeyword", value)
    }

    /* use parentheses */
    pub fn arrow_function_use_parentheses(&mut self, value: UseParentheses) -> &mut Self {
        self.insert("arrowFunction.useParentheses", value)
    }

    /* brace position */
    pub fn arrow_function_brace_position(&mut self, value: BracePosition) -> &mut Self {
        self.insert("arrowFunction.bracePosition", value)
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

    /* prefer hanging */
    pub fn arguments_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("arguments.preferHanging", value)
    }

    pub fn array_expression_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("arrayExpression.preferHanging", value)
    }

    pub fn array_pattern_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("arrayPattern.preferHanging", value)
    }

    pub fn do_while_statement_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("doWhileStatement.preferHanging", value)
    }

    pub fn export_declaration_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("exportDeclaration.preferHanging", value)
    }

    pub fn extends_clause_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("extendsClause.preferHanging", value)
    }

    pub fn for_in_statement_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("forInStatement.preferHanging", value)
    }

    pub fn for_of_statement_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("forOfStatement.preferHanging", value)
    }

    pub fn for_statement_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("forStatement.preferHanging", value)
    }

    pub fn if_statement_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("ifStatement.preferHanging", value)
    }

    pub fn implements_clause_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("implementsClause.preferHanging", value)
    }

    pub fn import_declaration_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("importDeclaration.preferHanging", value)
    }

    pub fn object_expression_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("objectExpression.preferHanging", value)
    }

    pub fn object_pattern_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("objectPattern.preferHanging", value)
    }

    pub fn parameters_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("parameters.preferHanging", value)
    }

    pub fn sequence_expression_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("sequenceExpression.preferHanging", value)
    }

    pub fn switch_statement_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("switchStatement.preferHanging", value)
    }

    pub fn tuple_type_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("tupleType.preferHanging", value)
    }

    pub fn type_literal_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("typeLiteral.preferHanging", value)
    }

    pub fn type_parameter_declaration_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("typeParameterDeclaration.preferHanging", value)
    }

    pub fn union_and_intersection_type_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("unionAndIntersectionType.preferHanging", value)
    }

    pub fn variable_statement_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("variableStatement.preferHanging", value)
    }

    pub fn while_statement_prefer_hanging(&mut self, value: bool) -> &mut Self {
        self.insert("whileStatement.preferHanging", value)
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

    pub fn arguments_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("arguments.trailingCommas", value)
    }

    pub fn parameters_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("parameters.trailingCommas", value)
    }

    pub fn array_expression_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("arrayExpression.trailingCommas", value)
    }

    pub fn array_pattern_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("arrayPattern.trailingCommas", value)
    }

    pub fn enum_declaration_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("enumDeclaration.trailingCommas", value)
    }

    pub fn export_declaration_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("exportDeclaration.trailingCommas", value)
    }

    pub fn import_declaration_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("importDeclaration.trailingCommas", value)
    }

    pub fn object_expression_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("objectExpression.trailingCommas", value)
    }

    pub fn object_pattern_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("objectPattern.trailingCommas", value)
    }

    pub fn tuple_type_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("tupleType.trailingCommas", value)
    }

    pub fn type_parameter_declaration_trailing_commas(&mut self, value: TrailingCommas) -> &mut Self {
        self.insert("typeParameterDeclaration.trailingCommas", value)
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

    /* prefer single line */

    pub fn arguments_prefer_single_line(&mut self, value: bool) -> &mut Self {
        self.insert("arguments.preferSingleLine", value)
    }

    pub fn conditional_expression_prefer_single_line(&mut self, value: bool) -> &mut Self {
        self.insert("conditionalExpression.preferSingleLine", value)
    }

    pub fn parameters_prefer_single_line(&mut self, value: bool) -> &mut Self {
        self.insert("parameters.preferSingleLine", value)
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

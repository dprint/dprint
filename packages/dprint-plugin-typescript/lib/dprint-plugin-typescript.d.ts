import { PrintItemIterable, WebAssemblyPlugin, PluginInitializeOptions, BaseResolvedConfiguration, ConfigurationDiagnostic } from "@dprint/types";

/**
 * User specified configuration for formatting TypeScript code.
 */
export interface TypeScriptConfiguration {
    /**
     * The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.
     * @default 120
     */
    lineWidth?: number;
    /**
     * The number of columns for an indent.
     * @default 4
     */
    indentWidth?: number;
    /**
     * Whether to use tabs (true) or spaces (false).
     * @default false
     */
    useTabs?: boolean;
    /**
     * Whether semi-colons should be used.
     * @default "prefer"
     * @value "always" - Always uses semi-colons where applicable.
     * @value "prefer" - Prefers to use semi-colons, but doesn't add one in certain scenarios
     * such as for the last member of a single-line type literal.
     * @value "asi" - Uses automatic semi-colon insertion. Only adds a semi-colon at the start
     * of some expression statements when necessary.
     */
    semiColons?: "always" | "prefer" | "asi";
    /**
     * How to decide to use single or double quotes.
     * @default "preferDouble"
     * @value "alwaysDouble" - Always use double quotes.
     * @value "alwaysSingle" - Always use single quotes.
     * @value "preferDouble" - Prefer using double quotes except in scenarios where the string
     * contains more double quotes than single quotes.
     * @value "preferSingle" - Prefer using single quotes except in scenarios where the string
     * contains more single quotes than double quotes.
     */
    quoteStyle?: "alwaysDouble" | "alwaysSingle" | "preferDouble" | "preferSingle";
    /**
     * The kind of newline to use.
     * @default "auto"
     * @value "auto" - For each file, uses the newline kind found at the end of the last line.
     * @value "crlf" - Uses carriage return, line feed.
     * @value "lf" - Uses line feed.
     * @value "system" - Uses the system standard (ex. crlf on Windows).
     */
    newLineKind?: "auto" | "crlf" | "lf" | "system";
    /**
     * If braces should be used or not.
     * @default "whenNotSingleLine"
     * @value "whenNotSingleLine" - Uses braces when the body is on a different line.
     * @value "maintain" - Uses braces if they're used. Doesn't use braces if they're not used.
     * @value "always" - Forces the use of braces. Will add them if they aren't used.
     * @value "preferNone" - Forces no braces when when the header is one line and body is one line. Otherwise forces braces.
     */
    useBraces?: "maintain" | "whenNotSingleLine" | "always" | "preferNone";
    /**
     * Where to place the opening brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    bracePosition?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
    /**
     * Where to place the expression of a statement that could possibly be on one line (ex. `if (true) console.log(5);`).
     * @default "maintain"
     * @value "maintain" - Maintains the position of the expression.
     * @value "sameLine" - Forces the whole statement to be on one line.
     * @value "nextLine" - Forces the expression to be on the next line.
     */
    singleBodyPosition?: "maintain" | "sameLine" | "nextLine";
    /**
     * Where to place the next control flow within a control flow statement.
     * @default "nextLine"
     * @value "maintain" - Maintains the next control flow being on the next line or the same line.
     * @value "sameLine" - Forces the next control flow to be on the same line.
     * @value "nextLine" - Forces the next control flow to be on the next line.
     */
    nextControlFlowPosition?: "maintain" | "sameLine" | "nextLine";
    /**
     * If trailing commas should be used.
     * @default "never"
     * @value "never" - Trailing commas should not be used.
     * @value "always" - Trailing commas should always be used.
     * @value "onlyMultiLine" - Trailing commas should only be used in multi-line scenarios.
     */
    trailingCommas?: "never" | "always" | "onlyMultiLine";
    /**
     * Where to place the operator for expressions that span multiple lines.
     * @default "nextLine"
     * @value "maintain" - Maintains the operator being on the next line or the same line.
     * @value "sameLine" - Forces the operator to be on the same line.
     * @value "nextLine" - Forces the operator to be on the next line.
     */
    operatorPosition?: "maintain" | "sameLine" | "nextLine";
    /**
     * Set to prefer hanging indentation when exceeding the line width.
     * @remarks When set, this value propagates down as the default value for
     * other configuration such as `preferHangingArguments` and
     * `preferHangingParameters`.
     * @default false
     */
    preferHanging?: boolean;
    /**
     * Prefers an argument list to be hanging when it exceeds the line width.
     * @remarks It will be hanging when the first argument is on the same line
     * as the open parenthesis and multi-line when on a different line.
     * @default false
     */
    preferHangingArguments?: boolean;
    /**
     * Forces a parameter list to be multi-line when it exceeds the line width.
     * @remarks It will be hanging when the first parameter is on the same line
     * as the open parenthesis and multi-line when on a different line.
     * @default false
     */
    preferHangingParameters?: boolean;
    /**
     * Whether to use parentheses around a single parameter in an arrow function.
     * @default "maintain"
     * @value "force" - Forces parentheses.
     * @value "maintain" - Maintains the current state of the parentheses.
     * @value "preferNone" - Prefers not using parentheses when possible.
     */
    "arrowFunctionExpression.useParentheses"?: "force" | "maintain" | "preferNone";
    /**
     * How to space the members of an enum.
     * @default "maintain"
     * @value "newline" - Forces a new line between members.
     * @value "blankline" - Forces a blank line between members.
     * @value "maintain" - Maintains whether a newline or blankline is used.
     */
    "enumDeclaration.memberSpacing"?: "newline" | "blankline" | "maintain";
    /**
     * Whether to surround the operator in a binary expression with spaces.
     * @default true
     * @value true - Ex. `1 + 2`
     * @value false - Ex. `1+2`
     */
    "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator"?: boolean;
    /**
     * Whether to add a space before the parentheses of a constructor.
     * @default false
     * @value true - Ex. `constructor ()`
     * @value false - Ex. `constructor()`
     */
    "constructor.spaceBeforeParentheses"?: boolean;
    /**
     * Whether to add a space after the `new` keyword in a constructor type.
     * @default false
     * @value true - Ex. `type MyClassCtor = new () => MyClass;`
     * @value false - Ex. `type MyClassCtor = new() => MyClass;`
     */
    "constructorType.spaceAfterNewKeyword"?: boolean;
    /**
     * Whether to add a space after the `new` keyword in a construct signature.
     * @default false
     * @value true - Ex. `new (): MyClass;`
     * @value false - Ex. `new(): MyClass;`
     */
    "constructSignature.spaceAfterNewKeyword"?: boolean;
    /**
     * Whether to add a space after the `while` keyword in a do while statement.
     * @default true
     * @value true - Ex. `do {\n} while (condition);`
     * @value false - Ex. `do {\n} while(condition);`
     */
    "doWhileStatement.spaceAfterWhileKeyword"?: boolean;
    /**
     * Whether to add spaces around named exports in an export declaration.
     * @default true
     * @value true - Ex. `export { SomeExport, OtherExport };`
     * @value false - Ex. `export {SomeExport, OtherExport};`
     */
    "exportDeclaration.spaceSurroundingNamedExports"?: boolean;
    /**
     * Whether to add a space after the `for` keyword in a "for in" statement.
     * @default true
     * @value true - Ex. `for (const prop in obj)`
     * @value false - Ex. `for(const prop in obj)`
     */
    "forInStatement.spaceAfterForKeyword"?: boolean;
    /**
     * Whether to add a space after the `for` keyword in a "for of" statement.
     * @default true
     * @value true - Ex. `for (const value of myArray)`
     * @value false - Ex. `for(const value of myArray)`
     */
    "forOfStatement.spaceAfterForKeyword"?: boolean;
    /**
     * Whether to add a space after the `for` keyword in a "for" statement.
     * @default true
     * @value true - Ex. `for (let i = 0; i < 5; i++)`
     * @value false - Ex. `for(let i = 0; i < 5; i++)`
     */
    "forStatement.spaceAfterForKeyword"?: boolean;
    /**
     * Whether to add a space after the semi-colons in a "for" statement.
     * @default true
     * @value true - Ex. `for (let i = 0; i < 5; i++)`
     * @value false - Ex. `for (let i = 0;i < 5;i++)`
     */
    "forStatement.spaceAfterSemiColons"?: boolean;
    /**
     * Whether to add a space before the parentheses of a function declaration.
     * @default false
     * @value true - Ex. `function myFunction ()`
     * @value false - Ex. `function myFunction()`
     */
    "functionDeclaration.spaceBeforeParentheses"?: boolean;
    /**
     * Whether to add a space before the parentheses of a function expression.
     * @default false
     * @value true - Ex. `function ()`
     * @value false - Ex. `function()`
     */
    "functionExpression.spaceBeforeParentheses"?: boolean;
    /**
     * Whether to add a space before the parentheses of a get accessor.
     * @default false
     * @value true - Ex. `get myProp ()`
     * @value false - Ex. `get myProp()`
     */
    "getAccessor.spaceBeforeParentheses"?: boolean;
    /**
     * Whether to add a space after the `if` keyword in an "if" statement.
     * @default true
     * @value true - Ex. `if (true)`
     * @value false - Ex. `if(true)`
     */
    "ifStatement.spaceAfterIfKeyword"?: boolean;
    /**
     * Whether to add spaces around named imports in an import declaration.
     * @default true
     * @value true - Ex. `import { SomeExport, OtherExport } from "my-module";`
     * @value false - Ex. `import {SomeExport, OtherExport} from "my-module";`
     */
    "importDeclaration.spaceSurroundingNamedImports"?: boolean;
    /**
     * Whether to add a space surrounding the expression of a JSX container.
     * @default false
     * @value true - Ex. `{ myValue }`
     * @value false - Ex. `{myValue}`
     */
    "jsxExpressionContainer.spaceSurroundingExpression"?: boolean;
    /**
     * Whether to add a space before the parentheses of a method.
     * @default false
     * @value true - Ex. `myMethod ()`
     * @value false - Ex. `myMethod()`
     */
    "method.spaceBeforeParentheses"?: boolean;
    /**
     * Whether to add a space before the parentheses of a set accessor.
     * @default false
     * @value true - Ex. `set myProp (value: string)`
     * @value false - Ex. `set myProp(value: string)`
     */
    "setAccessor.spaceBeforeParentheses"?: boolean;
    /**
     * Whether to add a space before the literal in a tagged templte.
     * @default true
     * @value true - Ex. `html \`<element />\``
     * @value false - Ex. `html\`<element />\``
     */
    "taggedTemplate.spaceBeforeLiteral"?: boolean;
    /**
     * Whether to add a space before the colon of a type annotation.
     * @default false
     * @value true - Ex. `function myFunction() : string`
     * @value false - Ex. `function myFunction(): string`
     */
    "typeAnnotation.spaceBeforeColon"?: boolean;
    /**
     * Whether to add a space before the expression in a type assertion.
     * @default true
     * @value true - Ex. `<string> myValue`
     * @value false - Ex. `<string>myValue`
     */
    "typeAssertion.spaceBeforeExpression"?: boolean;
    /**
     * Whether to add a space after the `while` keyword in a while statement.
     * @default true
     * @value true - Ex. `while (true)`
     * @value false - Ex. `while(true)`
     */
    "whileStatement.spaceAfterWhileKeyword"?: boolean;
    "forInStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "forOfStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "forStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "ifStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "whileStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "arrowFunctionExpression.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "classDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "classExpression.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "constructor.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "doWhileStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "enumDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "forInStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "forOfStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "forStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "functionDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "functionExpression.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "getAccessor.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "ifStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "interfaceDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "moduleDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "method.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "setAccessor.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "switchStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "switchCase.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "tryStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "whileStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "forInStatement.singleBodyPosition"?: TypeScriptConfiguration["singleBodyPosition"];
    "forOfStatement.singleBodyPosition"?: TypeScriptConfiguration["singleBodyPosition"];
    "forStatement.singleBodyPosition"?: TypeScriptConfiguration["singleBodyPosition"];
    "ifStatement.singleBodyPosition"?: TypeScriptConfiguration["singleBodyPosition"];
    "whileStatement.singleBodyPosition"?: TypeScriptConfiguration["singleBodyPosition"];
    "ifStatement.nextControlFlowPosition"?: TypeScriptConfiguration["nextControlFlowPosition"];
    "tryStatement.nextControlFlowPosition"?: TypeScriptConfiguration["nextControlFlowPosition"];
    "arrayExpression.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "arrayPattern.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "enumDeclaration.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "objectExpression.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "tupleType.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "binaryExpression.operatorPosition"?: TypeScriptConfiguration["operatorPosition"];
    "conditionalExpression.operatorPosition"?: TypeScriptConfiguration["operatorPosition"];
    "arrayExpression.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "arrayPattern.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "doWhileStatement.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "exportDeclaration.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "extendsClause.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "forInStatement.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "forOfStatement.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "forStatement.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "ifStatement.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "implementsClause.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "importDeclaration.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "objectExpression.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "objectPattern.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "sequenceExpression.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "switchStatement.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "tupleType.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "typeLiteral.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "typeParameterDeclaration.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "whileStatement.preferHanging"?: TypeScriptConfiguration["preferHanging"];
    "callExpression.preferHangingArguments"?: TypeScriptConfiguration["preferHangingArguments"];
    "newExpression.preferHangingArguments"?: TypeScriptConfiguration["preferHangingArguments"];
    "arrowFunctionExpression.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "callSignature.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "constructor.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "constructorType.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "constructSignature.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "functionDeclaration.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "functionExpression.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "functionType.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "getAccessor.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "method.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "methodSignature.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
    "setAccessor.preferHangingParameters"?: TypeScriptConfiguration["preferHangingParameters"];
}

/**
 * Resolved configuration from user specified configuration.
 */
export interface ResolvedTypeScriptConfiguration extends BaseResolvedConfiguration {
    readonly quoteStyle: NonNullable<TypeScriptConfiguration["quoteStyle"]>;
    readonly semiColons: NonNullable<TypeScriptConfiguration["semiColons"]>;
    readonly "forInStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "forOfStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "forStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "ifStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "whileStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "arrowFunctionExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "classDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "classExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "constructor.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "doWhileStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "enumDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "forInStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "forOfStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "forStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "functionDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "functionExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "getAccessor.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "ifStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "interfaceDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "method.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "moduleDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "setAccessor.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "switchStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "switchCase.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "tryStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "whileStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "forInStatement.singleBodyPosition": TypeScriptConfiguration["singleBodyPosition"];
    readonly "forOfStatement.singleBodyPosition": TypeScriptConfiguration["singleBodyPosition"];
    readonly "forStatement.singleBodyPosition": TypeScriptConfiguration["singleBodyPosition"];
    readonly "ifStatement.singleBodyPosition": TypeScriptConfiguration["singleBodyPosition"];
    readonly "whileStatement.singleBodyPosition": TypeScriptConfiguration["singleBodyPosition"];
    readonly "ifStatement.nextControlFlowPosition": NonNullable<TypeScriptConfiguration["nextControlFlowPosition"]>;
    readonly "tryStatement.nextControlFlowPosition": NonNullable<TypeScriptConfiguration["nextControlFlowPosition"]>;
    readonly "arrayExpression.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    readonly "arrayPattern.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    readonly "enumDeclaration.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    readonly "objectExpression.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    readonly "tupleType.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    readonly "binaryExpression.operatorPosition": NonNullable<TypeScriptConfiguration["operatorPosition"]>;
    readonly "conditionalExpression.operatorPosition": NonNullable<TypeScriptConfiguration["operatorPosition"]>;
    readonly "arrayExpression.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "arrayPattern.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "doWhileStatement.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "exportDeclaration.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "extendsClause.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "forInStatement.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "forOfStatement.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "forStatement.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "ifStatement.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "implementsClause.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "importDeclaration.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "objectExpression.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "objectPattern.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "sequenceExpression.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "switchStatement.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "tupleType.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "typeLiteral.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "typeParameterDeclaration.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "whileStatement.preferHanging": NonNullable<TypeScriptConfiguration["preferHanging"]>;
    readonly "callExpression.preferHangingArguments": NonNullable<TypeScriptConfiguration["preferHangingArguments"]>;
    readonly "newExpression.preferHangingArguments": NonNullable<TypeScriptConfiguration["preferHangingArguments"]>;
    readonly "arrowFunctionExpression.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "callSignature.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "constructor.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "constructorType.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "constructSignature.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "functionDeclaration.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "functionExpression.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "functionType.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "getAccessor.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "method.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "methodSignature.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "setAccessor.preferHangingParameters": NonNullable<TypeScriptConfiguration["preferHangingParameters"]>;
    readonly "arrowFunctionExpression.useParentheses": NonNullable<TypeScriptConfiguration["arrowFunctionExpression.useParentheses"]>;
    readonly "enumDeclaration.memberSpacing": NonNullable<TypeScriptConfiguration["enumDeclaration.memberSpacing"]>;
    readonly "binaryExpression.spaceSurroundingBitwiseAndArithmeticOperator": boolean;
    readonly "constructor.spaceBeforeParentheses": boolean;
    readonly "constructorType.spaceAfterNewKeyword": boolean;
    readonly "constructSignature.spaceAfterNewKeyword": boolean;
    readonly "doWhileStatement.spaceAfterWhileKeyword": boolean;
    readonly "exportDeclaration.spaceSurroundingNamedExports": boolean;
    readonly "forInStatement.spaceAfterForKeyword": boolean;
    readonly "forOfStatement.spaceAfterForKeyword": boolean;
    readonly "forStatement.spaceAfterForKeyword": boolean;
    readonly "forStatement.spaceAfterSemiColons": boolean;
    readonly "functionDeclaration.spaceBeforeParentheses": boolean;
    readonly "functionExpression.spaceBeforeParentheses": boolean;
    readonly "getAccessor.spaceBeforeParentheses": boolean;
    readonly "ifStatement.spaceAfterIfKeyword": boolean;
    readonly "importDeclaration.spaceSurroundingNamedImports": boolean;
    readonly "jsxExpressionContainer.spaceSurroundingExpression": boolean;
    readonly "method.spaceBeforeParentheses": boolean;
    readonly "setAccessor.spaceBeforeParentheses": boolean;
    readonly "taggedTemplate.spaceBeforeLiteral": boolean;
    readonly "typeAnnotation.spaceBeforeColon": boolean;
    readonly "typeAssertion.spaceBeforeExpression": boolean;
    readonly "whileStatement.spaceAfterWhileKeyword": boolean;
}

/**
 * Plugin for formatting TypeScript code (.ts/.tsx/.js/.jsx files).
 */
export declare class TypeScriptPlugin implements WebAssemblyPlugin<ResolvedTypeScriptConfiguration> {
    /**
     * Constructor.
     * @param config - The configuration to use.
     */
    constructor(config?: TypeScriptConfiguration);
    /** @inheritdoc */
    version: string;
    /** @inheritdoc */
    name: string;
    /** @inheritdoc */
    dispose(): void;
    /** @inheritdoc */
    initialize(options: PluginInitializeOptions): void;
    /** @inheritdoc */
    shouldFormatFile(filePath: string): boolean;
    /** @inheritdoc */
    getConfiguration(): ResolvedTypeScriptConfiguration;
    /** @inheritdoc */
    getConfigurationDiagnostics(): ConfigurationDiagnostic[];
    /** @inheritdoc */
    formatText(filePath: string, fileText: string): string | false;
    private _getFormatContext;
}

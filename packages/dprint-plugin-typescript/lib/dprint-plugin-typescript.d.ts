import { PrintItemIterable, Plugin, PluginInitializeOptions, BaseResolvedConfiguration, ConfigurationDiagnostic } from "@dprint/core";

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
     * The number of spaces for an indent. This option is ignored when using tabs.
     * @default 4
     */
    indentWidth?: number;
    /**
     * Whether to use tabs (true) or spaces (false).
     * @default false
     */
    useTabs?: boolean;
    /**
     * Whether statements should use semi-colons.
     * @default true
     */
    semiColons?: boolean;
    /**
     * Whether to use single quotes (true) or double quotes (false).
     * @default false
     */
    singleQuotes?: boolean;
    /**
     * The kind of newline to use.
     * @default "auto"
     * @value "auto" - For each file, uses the newline kind found at the end of the last line.
     * @value "crlf" - Uses carriage return, line feed.
     * @value "lf" - Uses line feed.
     * @value "system" - Uses the system standard (ex. crlf on Windows).
     */
    newlineKind?: "auto" | "crlf" | "lf" | "system";
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
     * Forces an argument list to be multi-line when it exceeds the print width.
     * @remarks - When false, it will be hanging when the first argument is on the same line
     * as the open parenthesis and multi-line when on a different line.
     * @default false
     */
    forceMultiLineArguments?: boolean;
    /**
     * Forces a parameter list to be multi-line when it exceeds the print width.
     * @remarks - When false, it will be hanging when the first parameter is on the same line
     * as the open parenthesis and multi-line when on a different line.
     * @default false
     */
    forceMultiLineParameters?: boolean;
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
    "binaryExpression.spaceSurroundingOperator"?: boolean;
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
     * Whether to add a space after the `if` keyword in an "if" statement.
     * @default true
     * @value true - Ex. `if (true)`
     * @value false - Ex. `if(true)`
     */
    "ifStatement.spaceAfterIfKeyword"?: boolean;
    /**
     * Whether to add spaces around named exports in an import declaration.
     * @default true
     * @value true - Ex. `import { SomeExport, OtherExport } from "my-module";`
     * @value false - Ex. `import {SomeExport, OtherExport} from "my-module";`
     */
    "importDeclaration.spaceSurroundingNamedExports"?: boolean;
    /**
     * Whether to add a space after the `while` keyword in a while statement.
     * @default true
     * @value true - Ex. `while (true)`
     * @value false - Ex. `while(true)`
     */
    "whileStatement.spaceAfterWhileKeyword"?: boolean;
    /**
     * Whether to add a space before the colon of a type annotation.
     * @default false
     * @value true - Ex. `function test() : string`
     * @value false - Ex. `function test(): string`
     */
    "typeAnnotation.spaceBeforeColon"?: boolean;
    /**
     * Whether to add a space before the expression in a type assertion.
     * @default true
     * @value true - Ex. `<string> myValue`
     * @value false - Ex. `<string>myValue`
     */
    "typeAssertion.spaceBeforeExpression"?: boolean;
    "breakStatement.semiColon"?: boolean;
    "callSignature.semiColon"?: boolean;
    "classMethod.semiColon"?: boolean;
    "classProperty.semiColon"?: boolean;
    "constructSignature.semiColon"?: boolean;
    "continueStatement.semiColon"?: boolean;
    "debuggerStatement.semiColon"?: boolean;
    "directive.semiColon"?: boolean;
    "doWhileStatement.semiColon"?: boolean;
    "exportAllDeclaration.semiColon"?: boolean;
    "exportAssignment.semiColon"?: boolean;
    "exportDefaultDeclaration.semiColon"?: boolean;
    "exportNamedDeclaration.semiColon"?: boolean;
    "expressionStatement.semiColon"?: boolean;
    "functionDeclaration.semiColon"?: boolean;
    "importDeclaration.semiColon"?: boolean;
    "importEqualsDeclaration.semiColon"?: boolean;
    "indexSignature.semiColon"?: boolean;
    "mappedType.semiColon"?: boolean;
    "methodSignature.semiColon"?: boolean;
    "moduleDeclaration.semiColon"?: boolean;
    "namespaceExportDeclaration.semiColon"?: boolean;
    "propertySignature.semiColon"?: boolean;
    "returnStatement.semiColon"?: boolean;
    "throwStatement.semiColon"?: boolean;
    "typeAlias.semiColon"?: boolean;
    "variableStatement.semiColon"?: boolean;
    "forInStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "forOfStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "forStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "ifStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "whileStatement.useBraces"?: TypeScriptConfiguration["useBraces"];
    "arrowFunctionExpression.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "classDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "classExpression.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "classMethod.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "doWhileStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "enumDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "forInStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "forOfStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "forStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "functionDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "functionExpression.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "ifStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "interfaceDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "moduleDeclaration.bracePosition"?: TypeScriptConfiguration["bracePosition"];
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
    "logicalExpression.operatorPosition"?: TypeScriptConfiguration["operatorPosition"];
    "callExpression.forceMultiLineArguments"?: TypeScriptConfiguration["forceMultiLineArguments"];
    "newExpression.forceMultiLineArguments"?: TypeScriptConfiguration["forceMultiLineArguments"];
    "arrowFunctionExpression.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "callSignature.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "classMethod.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "constructorType.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "constructSignature.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "functionDeclaration.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "functionExpression.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "functionType.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "methodSignature.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
    "objectMethod.forceMultiLineParameters"?: TypeScriptConfiguration["forceMultiLineParameters"];
}

/**
 * Resolved configuration from user specified configuration.
 */
export interface ResolvedTypeScriptConfiguration extends BaseResolvedConfiguration {
    readonly singleQuotes: boolean;
    readonly "breakStatement.semiColon": boolean;
    readonly "callSignature.semiColon": boolean;
    readonly "classMethod.semiColon": boolean;
    readonly "classProperty.semiColon": boolean;
    readonly "constructSignature.semiColon": boolean;
    readonly "continueStatement.semiColon": boolean;
    readonly "debuggerStatement.semiColon": boolean;
    readonly "directive.semiColon": boolean;
    readonly "doWhileStatement.semiColon": boolean;
    readonly "exportAllDeclaration.semiColon": boolean;
    readonly "exportAssignment.semiColon": boolean;
    readonly "exportDefaultDeclaration.semiColon": boolean;
    readonly "exportNamedDeclaration.semiColon": boolean;
    readonly "expressionStatement.semiColon": boolean;
    readonly "functionDeclaration.semiColon": boolean;
    readonly "importDeclaration.semiColon": boolean;
    readonly "importEqualsDeclaration.semiColon": boolean;
    readonly "indexSignature.semiColon": boolean;
    readonly "mappedType.semiColon": boolean;
    readonly "methodSignature.semiColon": boolean;
    readonly "moduleDeclaration.semiColon": boolean;
    readonly "namespaceExportDeclaration.semiColon": boolean;
    readonly "propertySignature.semiColon": boolean;
    readonly "returnStatement.semiColon": boolean;
    readonly "throwStatement.semiColon": boolean;
    readonly "typeAlias.semiColon": boolean;
    readonly "variableStatement.semiColon": boolean;
    readonly "forInStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "forOfStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "forStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "ifStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "whileStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    readonly "arrowFunctionExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "classDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "classExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "classMethod.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "doWhileStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "enumDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "forInStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "forOfStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "forStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "functionDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "functionExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "ifStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "interfaceDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    readonly "moduleDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
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
    readonly "logicalExpression.operatorPosition": NonNullable<TypeScriptConfiguration["operatorPosition"]>;
    readonly "callExpression.forceMultiLineArguments": NonNullable<TypeScriptConfiguration["forceMultiLineArguments"]>;
    readonly "newExpression.forceMultiLineArguments": NonNullable<TypeScriptConfiguration["forceMultiLineArguments"]>;
    readonly "arrowFunctionExpression.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "callSignature.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "classMethod.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "constructorType.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "constructSignature.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "functionDeclaration.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "functionExpression.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "functionType.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "methodSignature.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "objectMethod.forceMultiLineParameters": NonNullable<TypeScriptConfiguration["forceMultiLineParameters"]>;
    readonly "arrowFunctionExpression.useParentheses": NonNullable<TypeScriptConfiguration["arrowFunctionExpression.useParentheses"]>;
    readonly "enumDeclaration.memberSpacing": NonNullable<TypeScriptConfiguration["enumDeclaration.memberSpacing"]>;
    readonly "binaryExpression.spaceSurroundingOperator": boolean;
    readonly "constructorType.spaceAfterNewKeyword": boolean;
    readonly "constructSignature.spaceAfterNewKeyword": boolean;
    readonly "exportDeclaration.spaceSurroundingNamedExports": boolean;
    readonly "doWhileStatement.spaceAfterWhileKeyword": boolean;
    readonly "forInStatement.spaceAfterForKeyword": boolean;
    readonly "forOfStatement.spaceAfterForKeyword": boolean;
    readonly "forStatement.spaceAfterForKeyword": boolean;
    readonly "forStatement.spaceAfterSemiColons": boolean;
    readonly "functionDeclaration.spaceBeforeParentheses": boolean;
    readonly "functionExpression.spaceBeforeParentheses": boolean;
    readonly "ifStatement.spaceAfterIfKeyword": boolean;
    readonly "importDeclaration.spaceSurroundingNamedExports": boolean;
    readonly "whileStatement.spaceAfterWhileKeyword": boolean;
    readonly "typeAnnotation.spaceBeforeColon": boolean;
    readonly "typeAssertion.spaceBeforeExpression": boolean;
}

/**
 * Plugin for formatting TypeScript code (.ts/.tsx/.js files).
 */
export declare class TypeScriptPlugin implements Plugin<ResolvedTypeScriptConfiguration> {
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
    initialize(options: PluginInitializeOptions): void;
    /** @inheritdoc */
    shouldParseFile(filePath: string): boolean;
    /** @inheritdoc */
    getConfiguration(): ResolvedTypeScriptConfiguration;
    /** @inheritdoc */
    getConfigurationDiagnostics(): ConfigurationDiagnostic[];
    /** @inheritdoc */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false;
}

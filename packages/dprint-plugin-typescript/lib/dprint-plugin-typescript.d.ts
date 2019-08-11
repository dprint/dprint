import { PrintItemIterable, Plugin, PluginInitializeOptions, BaseResolvedConfiguration, ConfigurationDiagnostic } from "@dprint/core";

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
     * @default "maintain"
     * @value "maintain" - Uses braces if they're used. Doesn't use braces if they're not used.
     * @value "always" - Forces the use of braces. Will add them if they aren't used.
     * @value "preferNone" - Forces no braces when when the header is one line and body is one line. Otherwise forces braces.
     */
    useBraces?: "maintain" | "always" | "preferNone";
    /**
     * Where to place the brace.
     * @default "nextLineIfHanging"
     * @value "maintain" - Maintains the brace being on the next line or the same line.
     * @value "sameLine" - Forces the brace to be on the same line.
     * @value "nextLine" - Forces the brace to be on the next line.
     * @value "nextLineIfHanging" - Forces the brace to be on the next line if the same line is hanging, but otherwise uses the next.
     */
    bracePosition?: "maintain" | "sameLine" | "nextLine" | "nextLineIfHanging";
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
     * How to space the members of an enum.
     * @default "newline"
     * @value "newline" - Forces a new line between members.
     * @value "blankline" - Forces a blank line between members.
     * @value "maintain" - Maintains whether a newline or blankline is used.
     */
    "enumDeclaration.memberSpacing"?: "newline" | "blankline" | "maintain";
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
    "ifStatement.semiColon"?: boolean;
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
    "tryStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "whileStatement.bracePosition"?: TypeScriptConfiguration["bracePosition"];
    "ifStatement.nextControlFlowPosition"?: TypeScriptConfiguration["nextControlFlowPosition"];
    "tryStatement.nextControlFlowPosition"?: TypeScriptConfiguration["nextControlFlowPosition"];
    "arrayExpression.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "arrayPattern.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "enumDeclaration.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "objectExpression.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
    "tupleType.trailingCommas"?: TypeScriptConfiguration["trailingCommas"];
}

/**
 * Resolved configuration from user specified configuration.
 */
export interface ResolvedTypeScriptConfiguration extends BaseResolvedConfiguration {
    singleQuotes: boolean;
    newlineKind: "auto" | "\r\n" | "\n";
    lineWidth: NonNullable<TypeScriptConfiguration["lineWidth"]>;
    indentWidth: NonNullable<TypeScriptConfiguration["indentWidth"]>;
    useTabs: NonNullable<TypeScriptConfiguration["useTabs"]>;
    "enumDeclaration.memberSpacing": NonNullable<TypeScriptConfiguration["enumDeclaration.memberSpacing"]>;
    "breakStatement.semiColon": boolean;
    "callSignature.semiColon": boolean;
    "classMethod.semiColon": boolean;
    "classProperty.semiColon": boolean;
    "constructSignature.semiColon": boolean;
    "continueStatement.semiColon": boolean;
    "debuggerStatement.semiColon": boolean;
    "directive.semiColon": boolean;
    "doWhileStatement.semiColon": boolean;
    "exportAllDeclaration.semiColon": boolean;
    "exportAssignment.semiColon": boolean;
    "exportDefaultDeclaration.semiColon": boolean;
    "exportNamedDeclaration.semiColon": boolean;
    "expressionStatement.semiColon": boolean;
    "functionDeclaration.semiColon": boolean;
    "ifStatement.semiColon": boolean;
    "importDeclaration.semiColon": boolean;
    "importEqualsDeclaration.semiColon": boolean;
    "indexSignature.semiColon": boolean;
    "mappedType.semiColon": boolean;
    "methodSignature.semiColon": boolean;
    "moduleDeclaration.semiColon": boolean;
    "namespaceExportDeclaration.semiColon": boolean;
    "propertySignature.semiColon": boolean;
    "returnStatement.semiColon": boolean;
    "throwStatement.semiColon": boolean;
    "typeAlias.semiColon": boolean;
    "variableStatement.semiColon": boolean;
    "forInStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    "forOfStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    "forStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    "ifStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    "whileStatement.useBraces": NonNullable<TypeScriptConfiguration["useBraces"]>;
    "arrowFunctionExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "classDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "classExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "classMethod.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "doWhileStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "enumDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "forInStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "forOfStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "forStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "functionDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "functionExpression.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "ifStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "interfaceDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "moduleDeclaration.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "switchStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "tryStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "whileStatement.bracePosition": NonNullable<TypeScriptConfiguration["bracePosition"]>;
    "ifStatement.nextControlFlowPosition": NonNullable<TypeScriptConfiguration["nextControlFlowPosition"]>;
    "tryStatement.nextControlFlowPosition": NonNullable<TypeScriptConfiguration["nextControlFlowPosition"]>;
    "arrayExpression.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    "arrayPattern.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    "enumDeclaration.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    "objectExpression.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
    "tupleType.trailingCommas": NonNullable<TypeScriptConfiguration["trailingCommas"]>;
}

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

import * as os from "os";
import { ResolvedConfiguration, ConfigurationDiagnostic, ResolveConfigurationResult } from "@dprint/core";
import { TypeScriptConfiguration, ResolvedTypeScriptConfiguration } from "./Configuration";

/** todo: this should be code generated from the jsdocs maybe? */
const defaultValues = {
    semiColons: true,
    singleQuotes: false,
    useBraces: "whenNotSingleLine",
    bracePosition: "nextLineIfHanging",
    singleBodyPosition: "maintain",
    nextControlFlowPosition: "nextLine",
    operatorPosition: "nextLine",
    trailingCommas: "never",
    forceMultiLineArguments: false,
    forceMultiLineParameters: false,
    "enumDeclaration.memberSpacing": "maintain",
    "arrowFunctionExpression.useParentheses": "maintain",
    "binaryExpression.spaceSurroundingOperator": true,
    "constructor.spaceBeforeParentheses": false,
    "constructorType.spaceAfterNewKeyword": false,
    "constructSignature.spaceAfterNewKeyword": false,
    "doWhileStatement.spaceAfterWhileKeyword": true,
    "exportDeclaration.spaceSurroundingNamedExports": true,
    "forInStatement.spaceAfterForKeyword": true,
    "forOfStatement.spaceAfterForKeyword": true,
    "forStatement.spaceAfterForKeyword": true,
    "forStatement.spaceAfterSemiColons": true,
    "functionDeclaration.spaceBeforeParentheses": false,
    "functionExpression.spaceBeforeParentheses": false,
    "getAccessor.spaceBeforeParentheses": false,
    "ifStatement.spaceAfterIfKeyword": true,
    "importDeclaration.spaceSurroundingNamedExports": true,
    "jsxExpressionContainer.spaceSurroundingExpression": false,
    "method.spaceBeforeParentheses": false,
    "setAccessor.spaceBeforeParentheses": false,
    "typeAnnotation.spaceBeforeColon": false,
    "typeAssertion.spaceBeforeExpression": true,
    "whileStatement.spaceAfterWhileKeyword": true
} as const;

/**
 * Changes the provided configuration to have all its properties resolved to a value.
 * @param config - Configuration to resolve.
 */
export function resolveConfiguration(
    globalConfig: ResolvedConfiguration,
    pluginConfig: TypeScriptConfiguration
): ResolveConfigurationResult<ResolvedTypeScriptConfiguration> {
    pluginConfig = { ...pluginConfig };

    const diagnostics: ConfigurationDiagnostic[] = [];
    const semiColons = getValue("semiColons", defaultValues.semiColons, ensureBoolean);
    const useBraces = getValue("useBraces", defaultValues.useBraces, ensureBraceUse);
    const bracePosition = getValue("bracePosition", defaultValues.bracePosition, ensureBracePosition);
    const singleBodyPosition = getValue("singleBodyPosition", defaultValues.singleBodyPosition, ensureSingleBodyPosition);
    const nextControlFlowPosition = getValue("nextControlFlowPosition", defaultValues.nextControlFlowPosition, ensureNextControlFlowPosition);
    const operatorPosition = getValue("operatorPosition", defaultValues.operatorPosition, ensureOperatorPosition);
    const trailingCommas = getValue("trailingCommas", defaultValues.trailingCommas, ensureTrailingCommas);
    const forceMultiLineArguments = getValue("forceMultiLineArguments", defaultValues.forceMultiLineArguments, ensureBoolean);
    const forceMultiLineParameters = getValue("forceMultiLineParameters", defaultValues.forceMultiLineParameters, ensureBoolean);

    const resolvedConfig: ResolvedTypeScriptConfiguration = {
        singleQuotes: getValue("singleQuotes", defaultValues["singleQuotes"], ensureBoolean),
        newlineKind: getNewLineKind(),
        lineWidth: getValue("lineWidth", globalConfig.lineWidth, ensureNumber),
        indentWidth: getValue("indentWidth", globalConfig.indentWidth, ensureNumber),
        useTabs: getValue("useTabs", globalConfig.useTabs, ensureBoolean),
        // declaration specific
        "enumDeclaration.memberSpacing": getValue("enumDeclaration.memberSpacing", defaultValues["enumDeclaration.memberSpacing"], ensureEnumMemberSpacing),
        "arrowFunctionExpression.useParentheses": getValue("arrowFunctionExpression.useParentheses", defaultValues["arrowFunctionExpression.useParentheses"],
            ensureArrowFunctionUseParentheses),
        // semi-colons
        "breakStatement.semiColon": getValue("breakStatement.semiColon", semiColons, ensureBoolean),
        "callSignature.semiColon": getValue("callSignature.semiColon", semiColons, ensureBoolean),
        "classProperty.semiColon": getValue("classProperty.semiColon", semiColons, ensureBoolean),
        "constructor.semiColon": getValue("constructor.semiColon", semiColons, ensureBoolean),
        "constructSignature.semiColon": getValue("constructSignature.semiColon", semiColons, ensureBoolean),
        "continueStatement.semiColon": getValue("continueStatement.semiColon", semiColons, ensureBoolean),
        "debuggerStatement.semiColon": getValue("debuggerStatement.semiColon", semiColons, ensureBoolean),
        "directive.semiColon": getValue("directive.semiColon", semiColons, ensureBoolean),
        "doWhileStatement.semiColon": getValue("doWhileStatement.semiColon", semiColons, ensureBoolean),
        "exportAllDeclaration.semiColon": getValue("exportAllDeclaration.semiColon", semiColons, ensureBoolean),
        "exportAssignment.semiColon": getValue("exportAssignment.semiColon", semiColons, ensureBoolean),
        "exportDefaultDeclaration.semiColon": getValue("exportDefaultDeclaration.semiColon", semiColons, ensureBoolean),
        "exportNamedDeclaration.semiColon": getValue("exportNamedDeclaration.semiColon", semiColons, ensureBoolean),
        "expressionStatement.semiColon": getValue("expressionStatement.semiColon", semiColons, ensureBoolean),
        "functionDeclaration.semiColon": getValue("functionDeclaration.semiColon", semiColons, ensureBoolean),
        "getAccessor.semiColon": getValue("getAccessor.semiColon", semiColons, ensureBoolean),
        "importDeclaration.semiColon": getValue("importDeclaration.semiColon", semiColons, ensureBoolean),
        "importEqualsDeclaration.semiColon": getValue("importEqualsDeclaration.semiColon", semiColons, ensureBoolean),
        "indexSignature.semiColon": getValue("indexSignature.semiColon", semiColons, ensureBoolean),
        "mappedType.semiColon": getValue("mappedType.semiColon", semiColons, ensureBoolean),
        "method.semiColon": getValue("method.semiColon", semiColons, ensureBoolean),
        "methodSignature.semiColon": getValue("methodSignature.semiColon", semiColons, ensureBoolean),
        "moduleDeclaration.semiColon": getValue("moduleDeclaration.semiColon", semiColons, ensureBoolean),
        "namespaceExportDeclaration.semiColon": getValue("namespaceExportDeclaration.semiColon", semiColons, ensureBoolean),
        "propertySignature.semiColon": getValue("propertySignature.semiColon", semiColons, ensureBoolean),
        "returnStatement.semiColon": getValue("returnStatement.semiColon", semiColons, ensureBoolean),
        "setAccessor.semiColon": getValue("setAccessor.semiColon", semiColons, ensureBoolean),
        "throwStatement.semiColon": getValue("throwStatement.semiColon", semiColons, ensureBoolean),
        "typeAlias.semiColon": getValue("typeAlias.semiColon", semiColons, ensureBoolean),
        "variableStatement.semiColon": getValue("variableStatement.semiColon", semiColons, ensureBoolean),
        // useBraces
        "forInStatement.useBraces": getValue("forInStatement.useBraces", useBraces, ensureBraceUse),
        "forOfStatement.useBraces": getValue("forOfStatement.useBraces", useBraces, ensureBraceUse),
        "forStatement.useBraces": getValue("forStatement.useBraces", useBraces, ensureBraceUse),
        "ifStatement.useBraces": getValue("ifStatement.useBraces", useBraces, ensureBraceUse),
        "whileStatement.useBraces": getValue("whileStatement.useBraces", useBraces, ensureBraceUse),
        // bracePosition
        "arrowFunctionExpression.bracePosition": getValue("arrowFunctionExpression.bracePosition", bracePosition, ensureBracePosition),
        "classDeclaration.bracePosition": getValue("classDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "classExpression.bracePosition": getValue("classExpression.bracePosition", bracePosition, ensureBracePosition),
        "constructor.bracePosition": getValue("constructor.bracePosition", bracePosition, ensureBracePosition),
        "doWhileStatement.bracePosition": getValue("doWhileStatement.bracePosition", bracePosition, ensureBracePosition),
        "enumDeclaration.bracePosition": getValue("enumDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "forInStatement.bracePosition": getValue("forInStatement.bracePosition", bracePosition, ensureBracePosition),
        "forOfStatement.bracePosition": getValue("forOfStatement.bracePosition", bracePosition, ensureBracePosition),
        "forStatement.bracePosition": getValue("forStatement.bracePosition", bracePosition, ensureBracePosition),
        "functionDeclaration.bracePosition": getValue("functionDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "functionExpression.bracePosition": getValue("functionExpression.bracePosition", bracePosition, ensureBracePosition),
        "getAccessor.bracePosition": getValue("getAccessor.bracePosition", bracePosition, ensureBracePosition),
        "ifStatement.bracePosition": getValue("ifStatement.bracePosition", bracePosition, ensureBracePosition),
        "interfaceDeclaration.bracePosition": getValue("interfaceDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "moduleDeclaration.bracePosition": getValue("moduleDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "method.bracePosition": getValue("method.bracePosition", bracePosition, ensureBracePosition),
        "setAccessor.bracePosition": getValue("setAccessor.bracePosition", bracePosition, ensureBracePosition),
        "switchStatement.bracePosition": getValue("switchStatement.bracePosition", bracePosition, ensureBracePosition),
        "switchCase.bracePosition": getValue("switchCase.bracePosition", bracePosition, ensureBracePosition),
        "tryStatement.bracePosition": getValue("tryStatement.bracePosition", bracePosition, ensureBracePosition),
        "whileStatement.bracePosition": getValue("whileStatement.bracePosition", bracePosition, ensureBracePosition),
        // single body position
        "forInStatement.singleBodyPosition": getValue("forInStatement.singleBodyPosition", singleBodyPosition, ensureSingleBodyPosition),
        "forOfStatement.singleBodyPosition": getValue("forOfStatement.singleBodyPosition", singleBodyPosition, ensureSingleBodyPosition),
        "forStatement.singleBodyPosition": getValue("forStatement.singleBodyPosition", singleBodyPosition, ensureSingleBodyPosition),
        "ifStatement.singleBodyPosition": getValue("ifStatement.singleBodyPosition", singleBodyPosition, ensureSingleBodyPosition),
        "whileStatement.singleBodyPosition": getValue("whileStatement.singleBodyPosition", singleBodyPosition, ensureSingleBodyPosition),
        // next control flow position
        "ifStatement.nextControlFlowPosition": getValue("ifStatement.nextControlFlowPosition", nextControlFlowPosition, ensureNextControlFlowPosition),
        "tryStatement.nextControlFlowPosition": getValue("tryStatement.nextControlFlowPosition", nextControlFlowPosition, ensureNextControlFlowPosition),
        // operator position
        "binaryExpression.operatorPosition": getValue("binaryExpression.operatorPosition", operatorPosition, ensureOperatorPosition),
        "conditionalExpression.operatorPosition": getValue("conditionalExpression.operatorPosition", operatorPosition, ensureOperatorPosition),
        "logicalExpression.operatorPosition": getValue("logicalExpression.operatorPosition", operatorPosition, ensureOperatorPosition),
        // trailing commas
        "arrayExpression.trailingCommas": getValue("arrayExpression.trailingCommas", trailingCommas, ensureTrailingCommas),
        "arrayPattern.trailingCommas": getValue("arrayPattern.trailingCommas", trailingCommas, ensureTrailingCommas),
        "enumDeclaration.trailingCommas": getValue("enumDeclaration.trailingCommas", trailingCommas, ensureTrailingCommas),
        "objectExpression.trailingCommas": getValue("objectExpression.trailingCommas", trailingCommas, ensureTrailingCommas),
        "tupleType.trailingCommas": getValue("tupleType.trailingCommas", trailingCommas, ensureTrailingCommas),
        // force multi-line arguments
        "callExpression.forceMultiLineArguments": getValue("callExpression.forceMultiLineArguments", forceMultiLineArguments, ensureBoolean),
        "newExpression.forceMultiLineArguments": getValue("newExpression.forceMultiLineArguments", forceMultiLineArguments, ensureBoolean),
        // force multi-line parameters
        "arrowFunctionExpression.forceMultiLineParameters": getValue("arrowFunctionExpression.forceMultiLineParameters", forceMultiLineParameters,
            ensureBoolean),
        "callSignature.forceMultiLineParameters": getValue("callSignature.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "constructor.forceMultiLineParameters": getValue("constructor.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "constructorType.forceMultiLineParameters": getValue("constructorType.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "constructSignature.forceMultiLineParameters": getValue("constructSignature.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "functionDeclaration.forceMultiLineParameters": getValue("functionDeclaration.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "functionExpression.forceMultiLineParameters": getValue("functionExpression.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "functionType.forceMultiLineParameters": getValue("functionType.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "getAccessor.forceMultiLineParameters": getValue("getAccessor.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "method.forceMultiLineParameters": getValue("method.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "methodSignature.forceMultiLineParameters": getValue("methodSignature.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        "setAccessor.forceMultiLineParameters": getValue("setAccessor.forceMultiLineParameters", forceMultiLineParameters, ensureBoolean),
        // use spaces
        "binaryExpression.spaceSurroundingOperator": getValue("binaryExpression.spaceSurroundingOperator",
            defaultValues["binaryExpression.spaceSurroundingOperator"], ensureBoolean),
        "constructor.spaceBeforeParentheses": getValue("constructor.spaceBeforeParentheses", defaultValues["constructor.spaceBeforeParentheses"],
            ensureBoolean),
        "constructorType.spaceAfterNewKeyword": getValue("constructorType.spaceAfterNewKeyword", defaultValues["constructorType.spaceAfterNewKeyword"],
            ensureBoolean),
        "constructSignature.spaceAfterNewKeyword": getValue("constructSignature.spaceAfterNewKeyword",
            defaultValues["constructSignature.spaceAfterNewKeyword"], ensureBoolean),
        "doWhileStatement.spaceAfterWhileKeyword": getValue("doWhileStatement.spaceAfterWhileKeyword",
            defaultValues["doWhileStatement.spaceAfterWhileKeyword"], ensureBoolean),
        "exportDeclaration.spaceSurroundingNamedExports": getValue("exportDeclaration.spaceSurroundingNamedExports",
            defaultValues["exportDeclaration.spaceSurroundingNamedExports"], ensureBoolean),
        "forInStatement.spaceAfterForKeyword": getValue("forInStatement.spaceAfterForKeyword", defaultValues["forInStatement.spaceAfterForKeyword"],
            ensureBoolean),
        "forOfStatement.spaceAfterForKeyword": getValue("forOfStatement.spaceAfterForKeyword", defaultValues["forOfStatement.spaceAfterForKeyword"],
            ensureBoolean),
        "forStatement.spaceAfterForKeyword": getValue("forStatement.spaceAfterForKeyword", defaultValues["forStatement.spaceAfterForKeyword"], ensureBoolean),
        "forStatement.spaceAfterSemiColons": getValue("forStatement.spaceAfterSemiColons", defaultValues["forStatement.spaceAfterSemiColons"], ensureBoolean),
        "functionDeclaration.spaceBeforeParentheses": getValue("functionDeclaration.spaceBeforeParentheses",
            defaultValues["functionDeclaration.spaceBeforeParentheses"], ensureBoolean),
        "functionExpression.spaceBeforeParentheses": getValue("functionExpression.spaceBeforeParentheses",
            defaultValues["functionExpression.spaceBeforeParentheses"], ensureBoolean),
        "getAccessor.spaceBeforeParentheses": getValue("getAccessor.spaceBeforeParentheses", defaultValues["getAccessor.spaceBeforeParentheses"],
            ensureBoolean),
        "ifStatement.spaceAfterIfKeyword": getValue("ifStatement.spaceAfterIfKeyword", defaultValues["ifStatement.spaceAfterIfKeyword"], ensureBoolean),
        "importDeclaration.spaceSurroundingNamedExports": getValue("importDeclaration.spaceSurroundingNamedExports",
            defaultValues["importDeclaration.spaceSurroundingNamedExports"], ensureBoolean),
        "jsxExpressionContainer.spaceSurroundingExpression": getValue("jsxExpressionContainer.spaceSurroundingExpression",
            defaultValues["jsxExpressionContainer.spaceSurroundingExpression"], ensureBoolean),
        "method.spaceBeforeParentheses": getValue("method.spaceBeforeParentheses", defaultValues["method.spaceBeforeParentheses"], ensureBoolean),
        "setAccessor.spaceBeforeParentheses": getValue("setAccessor.spaceBeforeParentheses", defaultValues["setAccessor.spaceBeforeParentheses"],
            ensureBoolean),
        "typeAnnotation.spaceBeforeColon": getValue("typeAnnotation.spaceBeforeColon", defaultValues["typeAnnotation.spaceBeforeColon"], ensureBoolean),
        "typeAssertion.spaceBeforeExpression": getValue("typeAssertion.spaceBeforeExpression", defaultValues["typeAssertion.spaceBeforeExpression"],
            ensureBoolean),
        "whileStatement.spaceAfterWhileKeyword": getValue("whileStatement.spaceAfterWhileKeyword", defaultValues["whileStatement.spaceAfterWhileKeyword"],
            ensureBoolean)
    };

    addExcessPropertyDiagnostics();

    return {
        config: Object.freeze(resolvedConfig),
        diagnostics
    };

    function getNewLineKind() {
        const newlineKind = pluginConfig.newlineKind;
        delete pluginConfig.newlineKind;
        switch (newlineKind) {
            case "auto":
                return "auto";
            case "crlf":
                return "\r\n";
            case "lf":
                return "\n";
            case null:
            case undefined:
                return globalConfig.newlineKind;
            case "system":
                return os.EOL === "\r\n" ? "\r\n" : "\n";
            default:
                const propertyName: keyof TypeScriptConfiguration = "newlineKind";
                diagnostics.push({
                    propertyName,
                    message: `Unknown configuration specified for '${propertyName}': ${newlineKind}`
                });
                return globalConfig.newlineKind;
        }
    }

    function getValue<TKey extends keyof TypeScriptConfiguration>(
        key: TKey,
        defaultValue: NonNullable<TypeScriptConfiguration[TKey]>,
        validateFunc: (key: TKey, value: NonNullable<TypeScriptConfiguration[TKey]>) => boolean
    ) {
        let actualValue = pluginConfig[key] as NonNullable<TypeScriptConfiguration[TKey]>;
        if (actualValue == null || !validateFunc(key, actualValue as NonNullable<TypeScriptConfiguration[TKey]>))
            actualValue = defaultValue;

        delete pluginConfig[key];

        return actualValue;
    }

    function ensureNumber(key: keyof TypeScriptConfiguration, value: number) {
        if (typeof value === "number")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a number, but its value was: ${value}`
        });
        return false;
    }

    function ensureBoolean(key: keyof TypeScriptConfiguration, value: boolean) {
        if (typeof value === "boolean")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a boolean, but its value was: ${value}`
        });
        return false;
    }

    function ensureBraceUse(key: keyof TypeScriptConfiguration, value: TypeScriptConfiguration["useBraces"]) {
        switch (value) {
            case "maintain":
            case "whenNotSingleLine":
            case "preferNone":
            case "always":
            case null:
            case undefined:
                return true;
            default:
                const assertNever: never = value;
                diagnostics.push({
                    propertyName: key,
                    message: `Expected the configuration for '${key}' to equal one of the expected values, but was: ${value}`
                });
                return false;
        }
    }

    function ensureBracePosition(key: keyof TypeScriptConfiguration, value: TypeScriptConfiguration["bracePosition"]) {
        switch (value) {
            case "maintain":
            case "sameLine":
            case "nextLine":
            case "nextLineIfHanging":
            case null:
            case undefined:
                return true;
            default:
                const assertNever: never = value;
                diagnostics.push({
                    propertyName: key,
                    message: `Expected the configuration for '${key}' to equal one of the expected values, but was: ${value}`
                });
                return false;
        }
    }

    function ensureSingleBodyPosition(key: keyof TypeScriptConfiguration, value: TypeScriptConfiguration["singleBodyPosition"]) {
        switch (value) {
            case "maintain":
            case "sameLine":
            case "nextLine":
            case null:
            case undefined:
                return true;
            default:
                const assertNever: never = value;
                diagnostics.push({
                    propertyName: key,
                    message: `Expected the configuration for '${key}' to equal one of the expected values, but was: ${value}`
                });
                return false;
        }
    }

    function ensureNextControlFlowPosition(key: keyof TypeScriptConfiguration, value: TypeScriptConfiguration["nextControlFlowPosition"]) {
        switch (value) {
            case "maintain":
            case "sameLine":
            case "nextLine":
            case null:
            case undefined:
                return true;
            default:
                const assertNever: never = value;
                diagnostics.push({
                    propertyName: key,
                    message: `Expected the configuration for '${key}' to equal one of the expected values, but was: ${value}`
                });
                return false;
        }
    }

    function ensureOperatorPosition(key: keyof TypeScriptConfiguration, value: TypeScriptConfiguration["operatorPosition"]) {
        switch (value) {
            case "maintain":
            case "sameLine":
            case "nextLine":
            case null:
            case undefined:
                return true;
            default:
                const assertNever: never = value;
                diagnostics.push({
                    propertyName: key,
                    message: `Expected the configuration for '${key}' to equal one of the expected values, but was: ${value}`
                });
                return false;
        }
    }

    function ensureTrailingCommas(key: keyof TypeScriptConfiguration, value: TypeScriptConfiguration["trailingCommas"]) {
        switch (value) {
            case "never":
            case "always":
            case "onlyMultiLine":
            case null:
            case undefined:
                return true;
            default:
                const assertNever: never = value;
                diagnostics.push({
                    propertyName: key,
                    message: `Expected the configuration for '${key}' to equal one of the expected values, but was: ${value}`
                });
                return false;
        }
    }

    function ensureEnumMemberSpacing(key: keyof TypeScriptConfiguration, value: TypeScriptConfiguration["enumDeclaration.memberSpacing"]) {
        switch (value) {
            case "maintain":
            case "blankline":
            case "newline":
            case null:
            case undefined:
                return true;
            default:
                const assertNever: never = value;
                diagnostics.push({
                    propertyName: key,
                    message: `Expected the configuration for '${key}' to equal one of the expected values, but was: ${value}`
                });
                return false;
        }
    }

    function ensureArrowFunctionUseParentheses(key: keyof TypeScriptConfiguration, value: TypeScriptConfiguration["arrowFunctionExpression.useParentheses"]) {
        switch (value) {
            case "maintain":
            case "force":
            case "preferNone":
            case null:
            case undefined:
                return true;
            default:
                const assertNever: never = value;
                diagnostics.push({
                    propertyName: key,
                    message: `Expected the configuration for '${key}' to equal one of the expected values, but was: ${value}`
                });
                return false;
        }
    }

    function addExcessPropertyDiagnostics() {
        for (const propertyName in pluginConfig) {
            diagnostics.push({
                propertyName: propertyName as keyof typeof pluginConfig,
                message: `Unexpected property in configuration: ${propertyName}`
            });
        }
    }
}

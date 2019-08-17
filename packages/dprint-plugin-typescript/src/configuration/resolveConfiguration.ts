import * as os from "os";
import { ResolvedConfiguration, ConfigurationDiagnostic, ResolveConfigurationResult } from "@dprint/core";
import { TypeScriptConfiguration, ResolvedTypeScriptConfiguration } from "./Configuration";

/** Todo: this should be code generated from the jsdocs maybe? */
const defaultValues = {
    semiColons: true,
    singleQuotes: false,
    useBraces: "whenNotSingleLine",
    bracePosition: "nextLineIfHanging",
    singleBodyPosition: "maintain",
    nextControlFlowPosition: "nextLine",
    trailingCommas: "never",
    "enumDeclaration.memberSpacing": "maintain",
    "arrowFunctionExpression.useParentheses": "maintain"
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
    const trailingCommas = getValue("trailingCommas", defaultValues.trailingCommas, ensureTrailingCommas);

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
        "classMethod.semiColon": getValue("classMethod.semiColon", semiColons, ensureBoolean),
        "classProperty.semiColon": getValue("classProperty.semiColon", semiColons, ensureBoolean),
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
        "ifStatement.semiColon": getValue("ifStatement.semiColon", semiColons, ensureBoolean),
        "importDeclaration.semiColon": getValue("importDeclaration.semiColon", semiColons, ensureBoolean),
        "importEqualsDeclaration.semiColon": getValue("importEqualsDeclaration.semiColon", semiColons, ensureBoolean),
        "indexSignature.semiColon": getValue("indexSignature.semiColon", semiColons, ensureBoolean),
        "mappedType.semiColon": getValue("mappedType.semiColon", semiColons, ensureBoolean),
        "methodSignature.semiColon": getValue("methodSignature.semiColon", semiColons, ensureBoolean),
        "moduleDeclaration.semiColon": getValue("moduleDeclaration.semiColon", semiColons, ensureBoolean),
        "namespaceExportDeclaration.semiColon": getValue("namespaceExportDeclaration.semiColon", semiColons, ensureBoolean),
        "propertySignature.semiColon": getValue("propertySignature.semiColon", semiColons, ensureBoolean),
        "returnStatement.semiColon": getValue("returnStatement.semiColon", semiColons, ensureBoolean),
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
        "classMethod.bracePosition": getValue("classMethod.bracePosition", bracePosition, ensureBracePosition),
        "doWhileStatement.bracePosition": getValue("doWhileStatement.bracePosition", bracePosition, ensureBracePosition),
        "enumDeclaration.bracePosition": getValue("enumDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "forInStatement.bracePosition": getValue("forInStatement.bracePosition", bracePosition, ensureBracePosition),
        "forOfStatement.bracePosition": getValue("forOfStatement.bracePosition", bracePosition, ensureBracePosition),
        "forStatement.bracePosition": getValue("forStatement.bracePosition", bracePosition, ensureBracePosition),
        "functionDeclaration.bracePosition": getValue("functionDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "functionExpression.bracePosition": getValue("functionExpression.bracePosition", bracePosition, ensureBracePosition),
        "ifStatement.bracePosition": getValue("ifStatement.bracePosition", bracePosition, ensureBracePosition),
        "interfaceDeclaration.bracePosition": getValue("interfaceDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "moduleDeclaration.bracePosition": getValue("moduleDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "switchStatement.bracePosition": getValue("switchStatement.bracePosition", bracePosition, ensureBracePosition),
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
        // trailing commas
        "arrayExpression.trailingCommas": getValue("arrayExpression.trailingCommas", trailingCommas, ensureTrailingCommas),
        "arrayPattern.trailingCommas": getValue("arrayPattern.trailingCommas", trailingCommas, ensureTrailingCommas),
        "enumDeclaration.trailingCommas": getValue("enumDeclaration.trailingCommas", trailingCommas, ensureTrailingCommas),
        "objectExpression.trailingCommas": getValue("objectExpression.trailingCommas", trailingCommas, ensureTrailingCommas),
        "tupleType.trailingCommas": getValue("tupleType.trailingCommas", trailingCommas, ensureTrailingCommas)
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

import { Configuration, ResolvedConfiguration } from "./Configuration";
import * as os from "os";

/** The result of resolving configuration. */
export interface ResolveConfigurationResult {
    /** The resolved configuration. */
    config: ResolvedConfiguration;
    /** The diagnostics, if any. */
    diagnostics: ConfigurationDiagnostic[];
}

/** Represents a problem with a configuration. */
export interface ConfigurationDiagnostic {
    /** The property name the problem occurred on. */
    propertyName: string;
    /** The diagnostic's message. */
    message: string;
}

/** Do not edit. This variable's initializer is code generated from dprint.schema.json. */
const defaultValues = {
    lineWidth: 120,
    indentSize: 4,
    useTabs: false,
    semiColons: true,
    singleQuotes: false,
    newlineKind: "auto",
    useBraces: "maintain",
    bracePosition: "nextLineIfHanging",
    nextControlFlowPosition: "nextLine",
    trailingCommas: "never",
    "enumDeclaration.memberSpacing": "newline"
} as const;

/**
 * Changes the provided configuration to have all its properties resolved to a value.
 * @param config - Configuration to resolve.
 */
export function resolveConfiguration(config: Configuration): ResolveConfigurationResult {
    config = { ...config };
    const diagnostics: ConfigurationDiagnostic[] = [];
    const semiColons = getValue("semiColons", defaultValues["semiColons"], ensureBoolean);
    const useBraces = getValue("useBraces", defaultValues["useBraces"], ensureBraceUse);
    const bracePosition = getValue("bracePosition", defaultValues["bracePosition"], ensureBracePosition);
    const nextControlFlowPosition = getValue("nextControlFlowPosition", defaultValues["nextControlFlowPosition"], ensureNextControlFlowPosition);
    const trailingCommas = getValue("trailingCommas", defaultValues["trailingCommas"], ensureTrailingCommas);

    const resolvedConfig: ResolvedConfiguration = {
        lineWidth: getValue("lineWidth", defaultValues["lineWidth"], ensureNumber),
        indentSize: getValue("indentSize", defaultValues["indentSize"], ensureNumber),
        useTabs: getValue("useTabs", defaultValues["useTabs"], ensureBoolean),
        singleQuotes: getValue("singleQuotes", defaultValues["singleQuotes"], ensureBoolean),
        newlineKind: getNewLineKind(),
        // declaration specific
        "enumDeclaration.memberSpacing": getValue("enumDeclaration.memberSpacing", defaultValues["enumDeclaration.memberSpacing"], ensureEnumMemberSpacing),
        // semi-colons
        "breakStatement.semiColon": getValue("breakStatement.semiColon", semiColons, ensureBoolean),
        "classMethod.semiColon": getValue("classMethod.semiColon", semiColons, ensureBoolean),
        "classProperty.semiColon": getValue("classProperty.semiColon", semiColons, ensureBoolean),
        "continueStatement.semiColon": getValue("continueStatement.semiColon", semiColons, ensureBoolean),
        "debuggerStatement.semiColon": getValue("debuggerStatement.semiColon", semiColons, ensureBoolean),
        "directive.semiColon": getValue("directive.semiColon", semiColons, ensureBoolean),
        "doWhileStatement.semiColon": getValue("doWhileStatement.semiColon", semiColons, ensureBoolean),
        "exportAssignment.semiColon": getValue("exportAssignment.semiColon", semiColons, ensureBoolean),
        "expressionStatement.semiColon": getValue("expressionStatement.semiColon", semiColons, ensureBoolean),
        "functionDeclaration.semiColon": getValue("functionDeclaration.semiColon", semiColons, ensureBoolean),
        "ifStatement.semiColon": getValue("ifStatement.semiColon", semiColons, ensureBoolean),
        "importDeclaration.semiColon": getValue("importDeclaration.semiColon", semiColons, ensureBoolean),
        "importEqualsDeclaration.semiColon": getValue("importEqualsDeclaration.semiColon", semiColons, ensureBoolean),
        "mappedType.semiColon": getValue("mappedType.semiColon", semiColons, ensureBoolean),
        "returnStatement.semiColon": getValue("returnStatement.semiColon", semiColons, ensureBoolean),
        "throwStatement.semiColon": getValue("throwStatement.semiColon", semiColons, ensureBoolean),
        "typeAlias.semiColon": getValue("typeAlias.semiColon", semiColons, ensureBoolean),
        "variableStatement.semiColon": getValue("variableStatement.semiColon", semiColons, ensureBoolean),
        // useBraces
        "ifStatement.useBraces": getValue("ifStatement.useBraces", useBraces, ensureBraceUse),
        "whileStatement.useBraces": getValue("whileStatement.useBraces", useBraces, ensureBraceUse),
        // bracePosition
        "classDeclaration.bracePosition": getValue("classDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "classMethod.bracePosition": getValue("classMethod.bracePosition", bracePosition, ensureBracePosition),
        "doWhileStatement.bracePosition": getValue("doWhileStatement.bracePosition", bracePosition, ensureBracePosition),
        "enumDeclaration.bracePosition": getValue("enumDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "functionDeclaration.bracePosition": getValue("functionDeclaration.bracePosition", bracePosition, ensureBracePosition),
        "ifStatement.bracePosition": getValue("ifStatement.bracePosition", bracePosition, ensureBracePosition),
        "tryStatement.bracePosition": getValue("tryStatement.bracePosition", bracePosition, ensureBracePosition),
        "whileStatement.bracePosition": getValue("whileStatement.bracePosition", bracePosition, ensureBracePosition),
        // next control flow position
        "ifStatement.nextControlFlowPosition": getValue("ifStatement.nextControlFlowPosition", nextControlFlowPosition, ensureNextControlFlowPosition),
        "tryStatement.nextControlFlowPosition": getValue("tryStatement.nextControlFlowPosition", nextControlFlowPosition, ensureNextControlFlowPosition),
        // trailing commas
        "arrayExpression.trailingCommas": getValue("arrayExpression.trailingCommas", trailingCommas, ensureTrailingCommas),
        "enumDeclaration.trailingCommas": getValue("enumDeclaration.trailingCommas", trailingCommas, ensureTrailingCommas),
        "tupleType.trailingCommas": getValue("tupleType.trailingCommas", trailingCommas, ensureTrailingCommas)
    };

    addExcessPropertyDiagnostics();

    return {
        config: resolvedConfig,
        diagnostics
    };

    function getNewLineKind() {
        const newlineKind = config.newlineKind;
        delete config.newlineKind;
        switch (newlineKind) {
            case "auto":
                return "auto";
            case "crlf":
                return "\r\n";
            case "lf":
                return "\n";
            case null:
            case undefined:
                return defaultValues["newlineKind"];
            case "system":
                return os.EOL === "\r\n" ? "\r\n" : "\n";
            default:
                const propertyName = nameof<Configuration>(c => c.newlineKind);
                diagnostics.push({
                    propertyName,
                    message: `Unknown configuration specified for '${propertyName}': ${newlineKind}`
                });
                return defaultValues["newlineKind"];
        }
    }

    function getValue<TKey extends keyof Configuration>(
        key: TKey,
        defaultValue: NonNullable<Configuration[TKey]>,
        validateFunc: (key: string, value: NonNullable<Configuration[TKey]>) => boolean
    ) {
        let actualValue = config[key] as NonNullable<Configuration[TKey]>;
        if (actualValue == null || !validateFunc(key, actualValue as NonNullable<Configuration[TKey]>))
            actualValue = defaultValue;

        delete config[key];

        return actualValue;
    }

    function ensureNumber(key: string, value: number) {
        if (typeof value === "number")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a number, but its value was: ${value}`
        });
        return false;
    }

    function ensureBoolean(key: string, value: boolean) {
        if (typeof value === "boolean")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a boolean, but its value was: ${value}`
        });
        return false;
    }

    function ensureBraceUse(key: string, value: Configuration["useBraces"]) {
        switch (value) {
            case "maintain":
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

    function ensureBracePosition(key: string, value: Configuration["bracePosition"]) {
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

    function ensureNextControlFlowPosition(key: string, value: Configuration["nextControlFlowPosition"]) {
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

    function ensureTrailingCommas(key: string, value: Configuration["trailingCommas"]) {
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

    function ensureEnumMemberSpacing(key: string, value: Configuration["enumDeclaration.memberSpacing"]) {
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

    function addExcessPropertyDiagnostics() {
        for (const propertyName in config) {
            diagnostics.push({
                propertyName,
                message: `Unexpected property in configuration: ${propertyName}`
            });
        }
    }
}

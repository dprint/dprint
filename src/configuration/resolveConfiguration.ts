import { Configuration, ResolvedConfiguration } from "./Configuration";
import * as os from "os";

export interface ConfigurationDiagnostic {
    propertyName: string;
    message: string;
}

export interface ResolveConfigurationResult {
    /** The resolved configuration. */
    config: ResolvedConfiguration;
    /** The diagnostics, if any. */
    diagnostics: ConfigurationDiagnostic[];
}

/** Do not edit. This variable's initializer is code generated from dprint.schema.json. */
const defaultValues = {
    lineWidth: 120,
    indentSize: 4,
    useTabs: false,
    semiColons: true,
    singleQuotes: false,
    newLineKind: "auto",
    forceBraces: true
} as const;

export function resolveConfiguration(config: Configuration): ResolveConfigurationResult {
    config = { ...config };
    const diagnostics: ConfigurationDiagnostic[] = [];
    const semiColons = getValue("semiColons", defaultValues["semiColons"], ensureBoolean);
    const forceBraces = getValue("forceBraces", defaultValues["forceBraces"], ensureBoolean);

    const resolvedConfig: ResolvedConfiguration = {
        lineWidth: getValue("lineWidth", defaultValues["lineWidth"], ensureNumber),
        indentSize: getValue("indentSize", defaultValues["indentSize"], ensureNumber),
        useTabs: getValue("useTabs", defaultValues["useTabs"], ensureBoolean),
        singleQuotes: getValue("singleQuotes", defaultValues["singleQuotes"], ensureBoolean),
        newLineKind: getNewLineKind(),
        "directive.semiColon": getValue("directive.semiColon", semiColons, ensureBoolean),
        "doWhileStatement.semiColon": getValue("doWhileStatement.semiColon", semiColons, ensureBoolean),
        "expressionStatement.semiColon": getValue("expressionStatement.semiColon", semiColons, ensureBoolean),
        "ifStatement.semiColon": getValue("ifStatement.semiColon", semiColons, ensureBoolean),
        "importDeclaration.semiColon": getValue("importDeclaration.semiColon", semiColons, ensureBoolean),
        "typeAlias.semiColon": getValue("typeAlias.semiColon", semiColons, ensureBoolean),
        "ifStatement.forceBraces": getValue("ifStatement.forceBraces", forceBraces, ensureBoolean)
    };

    addExcessPropertyDiagnostics();

    return {
        config: resolvedConfig,
        diagnostics
    };

    function getNewLineKind() {
        const newLineKind = config.newLineKind;
        delete config.newLineKind;
        switch (newLineKind) {
            case "auto":
                return "auto";
            case "crlf":
                return "\r\n";
            case "lf":
                return "\n";
            case null:
            case undefined:
                return defaultValues["newLineKind"];
            case "system":
                return os.EOL === "\r\n" ? "\r\n" : "\n";
            default:
                const propertyName = nameof<Configuration>(c => c.newLineKind);
                diagnostics.push({
                    propertyName,
                    message: `Unknown configuration specified for '${propertyName}': ${newLineKind}`
                });
                return defaultValues["newLineKind"];
        }
    }

    function getValue<TKey extends keyof Configuration>(
        key: TKey,
        defaultValue: NonNullable<Configuration[TKey]>,
        validateFunc: (key: string, value: NonNullable<Configuration[TKey]>) => void
    ) {
        let actualValue = config[key] as NonNullable<Configuration[TKey]>;
        if (actualValue == null)
            actualValue = defaultValue;
        else
            validateFunc(key, actualValue as NonNullable<Configuration[TKey]>);

        delete config[key];

        return actualValue;
    }

    function ensureNumber(key: string, value: number) {
        if (typeof value === "number")
            return;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a number, but its value was: ${value}`
        });
    }

    function ensureBoolean(key: string, value: boolean) {
        if (typeof value === "boolean")
            return;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a boolean, but its value was: ${value}`
        });
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

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

export function resolveConfiguration(config: Configuration): ResolveConfigurationResult {
    config = { ...config };
    const diagnostics: ConfigurationDiagnostic[] = [];
    const semiColons = getValue("semiColons", true, ensureBoolean);

    const resolvedConfig: ResolvedConfiguration = {
        "ifStatement.semiColon": getValue("ifStatement.semiColon", semiColons, ensureBoolean),
        singleQuotes: getValue("singleQuotes", false, ensureBoolean),
        newLineKind: getNewLineKind()
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
            case "crlf":
            case "lf":
                return newLineKind;
            case null:
            case undefined:
                return "auto";
            case "system":
                return os.EOL === "\r\n" ? "crlf" : "lf";
            default:
                const propertyName = nameof<Configuration>(c => c.newLineKind);
                diagnostics.push({
                    propertyName,
                    message: `Unknown configuration specified for '${propertyName}': ${newLineKind}`
                });
                return "auto";
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

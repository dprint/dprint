import * as os from "os";
import { Configuration, ResolvedConfiguration, ConfigurationDiagnostic } from "@dprint/types";
import { ResolveConfigurationResult } from "./ResolveConfigurationResult";

const defaultValues = {
    lineWidth: 120,
    indentWidth: 4,
    useTabs: false,
    newLineKind: "lf",
} as const;

/**
 * Changes the provided configuration to have all its properties resolved to a value.
 * @param config - Configuration to resolve.
 * @param pluginPropertyNames - Collection of plugin property names to ignore for excess property diagnostics.
 */
export function resolveConfiguration(config: Partial<Configuration>): ResolveConfigurationResult<ResolvedConfiguration> {
    config = { ...config };
    const diagnostics: ConfigurationDiagnostic[] = [];

    const resolvedConfig: ResolvedConfiguration = {
        lineWidth: getValue("lineWidth", defaultValues.lineWidth, ensureNumber),
        indentWidth: getValue("indentWidth", defaultValues.indentWidth, ensureNumber),
        useTabs: getValue("useTabs", defaultValues.useTabs, ensureBoolean),
        newLineKind: getNewLineKind(),
    };

    addExcessPropertyDiagnostics();

    return {
        config: resolvedConfig,
        diagnostics,
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
                return defaultValues.newLineKind;
            case "system":
                return os.EOL === "\r\n" ? "crlf" : "lf";
            default:
                const propertyName: keyof ResolvedConfiguration = "newLineKind";
                diagnostics.push({
                    propertyName,
                    message: `Unknown configuration specified for '${propertyName}': ${newLineKind}`,
                });
                return defaultValues["newLineKind"];
        }
    }

    function getValue<TKey extends keyof Configuration>(
        key: TKey,
        defaultValue: NonNullable<Configuration[TKey]>,
        validateFunc: (key: TKey, value: NonNullable<Configuration[TKey]>) => boolean,
    ) {
        let actualValue = config[key] as NonNullable<Configuration[TKey]>;
        if (actualValue == null || !validateFunc(key, actualValue as NonNullable<Configuration[TKey]>))
            actualValue = defaultValue;

        delete config[key];

        return actualValue;
    }

    function ensureNumber(key: keyof Configuration, value: number) {
        if (typeof value === "number")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a number, but its value was: ${value}`,
        });
        return false;
    }

    function ensureBoolean(key: keyof Configuration, value: boolean) {
        if (typeof value === "boolean")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a boolean, but its value was: ${value}`,
        });
        return false;
    }

    function addExcessPropertyDiagnostics() {
        for (const propertyName in config) {
            if (propertyName === nameof<Configuration>(c => c.projectType)
                || propertyName === nameof<Configuration>(c => c.plugins))
            {
                continue;
            }

            diagnostics.push({
                propertyName: propertyName as keyof typeof config,
                message: `Unknown property in configuration: ${propertyName}`,
            });
        }
    }
}

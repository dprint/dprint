import * as os from "os";
import { ResolvedConfiguration, ConfigurationDiagnostic } from "@dprint/types";
import { ResolveConfigurationResult } from "@dprint/core";
import { JsoncConfiguration, ResolvedJsoncConfiguration } from "./Configuration";

/**
 * Gets a resolved configuration from the provided global and plugin configuration.
 * @param config - Configuration to resolve.
 */
export function resolveConfiguration(
    globalConfig: ResolvedConfiguration,
    pluginConfig: JsoncConfiguration
): ResolveConfigurationResult<ResolvedJsoncConfiguration> {
    pluginConfig = { ...pluginConfig };

    const diagnostics: ConfigurationDiagnostic[] = [];

    const resolvedConfig: ResolvedJsoncConfiguration = {
        newLineKind: getNewLineKind(),
        lineWidth: getValue("lineWidth", globalConfig.lineWidth, ensureNumber),
        indentWidth: getValue("indentWidth", globalConfig.indentWidth, ensureNumber),
        useTabs: getValue("useTabs", globalConfig.useTabs, ensureBoolean)
    };

    addExcessPropertyDiagnostics();

    return {
        config: Object.freeze(resolvedConfig),
        diagnostics
    };

    function getNewLineKind() {
        const newLineKind = pluginConfig.newLineKind;
        delete pluginConfig.newLineKind;
        switch (newLineKind) {
            case "auto":
                return "auto";
            case "crlf":
                return "\r\n";
            case "lf":
                return "\n";
            case null:
            case undefined:
                return globalConfig.newLineKind;
            case "system":
                return os.EOL === "\r\n" ? "\r\n" : "\n";
            default:
                const propertyName: keyof JsoncConfiguration = "newLineKind";
                diagnostics.push({
                    propertyName,
                    message: `Unknown configuration specified for '${propertyName}': ${newLineKind}`
                });
                return globalConfig.newLineKind;
        }
    }

    function getValue<TKey extends keyof JsoncConfiguration>(
        key: TKey,
        defaultValue: NonNullable<JsoncConfiguration[TKey]>,
        validateFunc: (key: TKey, value: NonNullable<JsoncConfiguration[TKey]>) => boolean
    ) {
        let actualValue = pluginConfig[key] as NonNullable<JsoncConfiguration[TKey]>;
        if (actualValue == null || !validateFunc(key, actualValue as NonNullable<JsoncConfiguration[TKey]>))
            actualValue = defaultValue;

        delete pluginConfig[key];

        return actualValue;
    }

    function ensureNumber(key: keyof JsoncConfiguration, value: number) {
        if (typeof value === "number")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a number, but its value was: ${value}`
        });
        return false;
    }

    function ensureBoolean(key: keyof JsoncConfiguration, value: boolean) {
        if (typeof value === "boolean")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a boolean, but its value was: ${value}`
        });
        return false;
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

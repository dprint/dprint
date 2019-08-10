import * as os from "os";
import { GlobalConfiguration, ResolvedGlobalConfiguration } from "./GlobalConfiguration";
import { ConfigurationDiagnostic } from "./ConfigurationDiagnostic";
import { ResolveConfigurationResult } from "./ResolveConfigurationResult";

// todo: make this code generated again? or convert to not using a configuration file
/** Do not edit. This variable's initializer is code generated from dprint.schema.json. */
const defaultValues = {
    lineWidth: 120,
    indentWidth: 4,
    useTabs: false,
    newlineKind: "auto"
} as const;

/**
 * Changes the provided configuration to have all its properties resolved to a value.
 * @param config - Configuration to resolve.
 * @param pluginPropertyNames - Collection of plugin property names to ignore for excess property diagnostics.
 */
export function resolveGlobalConfiguration(config: GlobalConfiguration, pluginPropertyNames: string[]): ResolveConfigurationResult<ResolvedGlobalConfiguration> {
    config = { ...config };
    const diagnostics: ConfigurationDiagnostic[] = [];

    const resolvedConfig: ResolvedGlobalConfiguration = {
        lineWidth: getValue("lineWidth", defaultValues["lineWidth"], ensureNumber),
        indentWidth: getValue("indentWidth", defaultValues["indentWidth"], ensureNumber),
        useTabs: getValue("useTabs", defaultValues["useTabs"], ensureBoolean),
        newlineKind: getNewLineKind()
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
                const propertyName: keyof ResolvedGlobalConfiguration = "newlineKind";
                diagnostics.push({
                    propertyName,
                    message: `Unknown configuration specified for '${propertyName}': ${newlineKind}`
                });
                return defaultValues["newlineKind"];
        }
    }

    function getValue<TKey extends keyof GlobalConfiguration>(
        key: TKey,
        defaultValue: NonNullable<GlobalConfiguration[TKey]>,
        validateFunc: (key: TKey, value: NonNullable<GlobalConfiguration[TKey]>) => boolean
    ) {
        let actualValue = config[key] as NonNullable<GlobalConfiguration[TKey]>;
        if (actualValue == null || !validateFunc(key, actualValue as NonNullable<GlobalConfiguration[TKey]>))
            actualValue = defaultValue;

        delete config[key];

        return actualValue;
    }

    function ensureNumber(key: keyof GlobalConfiguration, value: number) {
        if (typeof value === "number")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a number, but its value was: ${value}`
        });
        return false;
    }

    function ensureBoolean(key: keyof GlobalConfiguration, value: boolean) {
        if (typeof value === "boolean")
            return true;

        diagnostics.push({
            propertyName: key,
            message: `Expected the configuration for '${key}' to be a boolean, but its value was: ${value}`
        });
        return false;
    }

    function addExcessPropertyDiagnostics() {
        for (const propertyName in config) {
            if (propertyName === nameof<GlobalConfiguration>(c => c.projectType))
                continue;
            // ignore plugin property names
            if (pluginPropertyNames.includes(propertyName))
                continue;

            diagnostics.push({
                propertyName: propertyName as keyof typeof config,
                message: `Unexpected property in configuration: ${propertyName}`
            });
        }
    }
}

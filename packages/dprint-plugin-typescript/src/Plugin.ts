import { getFileExtension, CliLoggingEnvironment } from "@dprint/core";
import { WebAssemblyPlugin, ConfigurationDiagnostic, PluginInitializeOptions, LoggingEnvironment, ResolvedConfiguration as GlobalConfig } from "@dprint/types";
import { TypeScriptConfiguration, ResolvedTypeScriptConfiguration } from "./Configuration";
import { FormatContext } from "./wasm/ts_dprint_plugin_typescript";

/**
 * Plugin for formatting TypeScript code (.ts/.tsx/.js/.jsx files).
 */
export class TypeScriptPlugin implements WebAssemblyPlugin<ResolvedTypeScriptConfiguration> {
    /** @internal */
    private readonly _typeScriptConfig: TypeScriptConfiguration;
    /** @internal */
    private _globalConfig: GlobalConfig | undefined;
    /** @internal */
    private _environment?: LoggingEnvironment;
    /** @internal */
    private _formatContext: FormatContext | undefined;

    /**
     * Constructor.
     * @param config - The configuration to use.
     */
    constructor(config: TypeScriptConfiguration = {}) {
        this._typeScriptConfig = config;
    }

    /** @inheritdoc */
    version = "PACKAGE_VERSION"; // value is replaced at build time

    /** @inheritdoc */
    name = "dprint-plugin-typescript";

    /** @inheritdoc */
    dispose() {
        this._formatContext?.free();
    }

    /** @inheritdoc */
    initialize(options: PluginInitializeOptions) {
        this._formatContext?.free();
        this._formatContext = undefined;

        this._globalConfig = options.globalConfig;
        this._environment = options.environment;
    }

    /** @inheritdoc */
    shouldFormatFile(filePath: string) {
        switch (getFileExtension(filePath).toLowerCase()) {
            case ".ts":
            case ".tsx":
            case ".js":
            case ".jsx":
                return true;
            default:
                return false;
        }
    }

    /** @inheritdoc */
    getConfiguration() {
        return JSON.parse(this._getFormatContext().get_configuration()) as ResolvedTypeScriptConfiguration;
    }

    /** @inheritdoc */
    getConfigurationDiagnostics() {
        return JSON.parse(this._getFormatContext().get_configuration_diagnostics()) as ConfigurationDiagnostic[];
    }

    /** @inheritdoc */
    formatText(filePath: string, fileText: string): string | false {
        const result = this._getFormatContext().format(filePath, fileText);
        if (result == null)
            return false;
        return result;
    }

    private _getFormatContext() {
        return this._formatContext ?? (this._formatContext = FormatContext.new(this._getConfigMap()));
    }

    /** @internal */
    private _getConfigMap() {
        const map = new Map();
        if (this._globalConfig)
            addConfigToMap(this._globalConfig);
        addConfigToMap(this._typeScriptConfig);
        return map;

        function addConfigToMap(config: any) {
            for (let key of Object.keys(config)) {
                const value = config[key] as unknown;
                if (value == null)
                    continue;
                else if (typeof value === "string" || typeof value === "boolean" || typeof value === "number")
                    map.set(key, value.toString());
                else
                    throw new Error(`Not supported value type '${typeof value}' for key '${key}'.`);
            }
        }
    }

    /** @internal */
    private _getEnvironment() {
        if (this._environment == null)
            this._environment = new CliLoggingEnvironment();
        return this._environment;
    }
}

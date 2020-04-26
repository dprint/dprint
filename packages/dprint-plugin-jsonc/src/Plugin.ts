import { getFileExtension, CliLoggingEnvironment } from "@dprint/core";
import { WebAssemblyPlugin, ConfigurationDiagnostic, PluginInitializeOptions, LoggingEnvironment, ResolvedConfiguration as GlobalConfig } from "@dprint/types";
import { JsoncConfiguration, ResolvedJsoncConfiguration } from "./Configuration";
import { FormatContext } from "./wasm/ts_dprint_plugin_jsonc";

export class JsoncPlugin implements WebAssemblyPlugin<ResolvedJsoncConfiguration> {
    /** @internal */
    private readonly _jsoncConfig: JsoncConfiguration;
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
    constructor(config: JsoncConfiguration = {}) {
        this._jsoncConfig = config;
    }

    /** @inheritdoc */
    version = "PACKAGE_VERSION"; // value is replaced at build time

    /** @inheritdoc */
    name = "dprint-plugin-json";

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
        return getFileExtension(filePath).toLowerCase() === ".json";
    }

    /** @inheritdoc */
    getConfiguration() {
        return JSON.parse(this._getFormatContext().get_configuration()) as ResolvedJsoncConfiguration;
    }

    /** @inheritdoc */
    getConfigurationDiagnostics() {
        return JSON.parse(this._getFormatContext().get_configuration_diagnostics()) as ConfigurationDiagnostic[];
    }

    /** @inheritdoc */
    formatText(filePath: string, fileText: string): string | false {
        const result = this._getFormatContext().format(fileText);
        if (result == null)
            return false;
        return result;
    }

    private _getFormatContext() {
        return this._formatContext ?? (this._formatContext = FormatContext.new(
            objectToMap(this._jsoncConfig),
            objectToMap(this._globalConfig),
        ));
    }

    /** @internal */
    private _getEnvironment() {
        if (this._environment == null)
            this._environment = new CliLoggingEnvironment();
        return this._environment;
    }
}

function objectToMap(config: any) {
    const map = new Map();
    for (let key of Object.keys(config)) {
        const value = config[key] as unknown;
        if (value == null)
            continue;
        else if (typeof value === "string" || typeof value === "boolean" || typeof value === "number")
            map.set(key, value.toString());
        else
            throw new Error(`Not supported value type '${typeof value}' for key '${key}'.`);
    }
    return map;
}

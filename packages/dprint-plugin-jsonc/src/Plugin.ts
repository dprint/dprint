import { Plugin, getFileExtension, ResolveConfigurationResult, PrintItemIterable, ConfigurationDiagnostic, resolveConfiguration as resolveGlobalConfiguration,
    PluginInitializeOptions, LoggingEnvironment, CliLoggingEnvironment } from "@dprint/core";
import { JsoncConfiguration, ResolvedJsoncConfiguration, resolveConfiguration } from "./configuration";
import { parseToJsonAst, parseJsonFile } from "./parser";

export class JsoncPlugin implements Plugin<ResolvedJsoncConfiguration> {
    /** @internal */
    private readonly _unresolvedConfig: JsoncConfiguration;
    /** @internal */
    private _resolveConfigurationResult?: ResolveConfigurationResult<ResolvedJsoncConfiguration>;
    /** @internal */
    private _environment?: LoggingEnvironment;

    /**
     * Constructor.
     * @param config - The configuration to use.
     */
    constructor(config: JsoncConfiguration = {}) {
        this._unresolvedConfig = config;
    }

    /** @inheritdoc */
    version = "PACKAGE_VERSION"; // value is replaced at build time

    /** @inheritdoc */
    name = "dprint-plugin-json";

    /** @inheritdoc */
    initialize(options: PluginInitializeOptions) {
        this._resolveConfigurationResult = resolveConfiguration(options.globalConfig, this._unresolvedConfig);
        this._environment = options.environment;
    }

    /** @inheritdoc */
    shouldParseFile(filePath: string) {
        return getFileExtension(filePath).toLowerCase() === ".json";
    }

    /** @inheritdoc */
    getConfiguration(): ResolvedJsoncConfiguration {
        return this._getResolveConfigurationResult().config;
    }

    /** @inheritdoc */
    getConfigurationDiagnostics(): ConfigurationDiagnostic[] {
        return this._getResolveConfigurationResult().diagnostics;
    }

    /** @inheritdoc */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false {
        const jsonAst = parseToJsonAst(fileText);
        return parseJsonFile({
            file: jsonAst,
            filePath,
            fileText,
            config: this.getConfiguration(),
            environment: this._getEnvironment()
        });
    }

    /** @internal */
    private _getResolveConfigurationResult() {
        if (this._resolveConfigurationResult == null) {
            const globalConfig = resolveGlobalConfiguration({}).config;
            this._resolveConfigurationResult = resolveConfiguration(globalConfig, this._unresolvedConfig);
        }
        return this._resolveConfigurationResult;
    }

    /** @internal */
    private _getEnvironment() {
        if (this._environment == null)
            this._environment = new CliLoggingEnvironment();
        return this._environment;
    }
}

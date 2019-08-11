import { Plugin, getFileExtension, ResolveConfigurationResult, ResolvedConfiguration, PrintItemIterable, ConfigurationDiagnostic } from "@dprint/core";
import { JsoncConfiguration, ResolvedJsoncConfiguration, resolveConfiguration } from "./configuration";
import { parseToJsonAst, parseJsonFile } from "./parser";
import { throwError } from "./utils";

export class JsoncPlugin implements Plugin<ResolvedJsoncConfiguration> {
    /** @internal */
    private readonly _unresolvedConfig: JsoncConfiguration;
    /** @internal */
    private _resolveConfigurationResult?: ResolveConfigurationResult<ResolvedJsoncConfiguration>;

    /**
     * Constructor.
     * @param config - The configuration to use.
     */
    constructor(config: JsoncConfiguration) {
        this._unresolvedConfig = config;
    }

    /** @inheritdoc */
    version = "PACKAGE_VERSION"; // value is replaced at build time

    /** @inheritdoc */
    name = "dprint-plugin-json";

    /** @inheritdoc */
    shouldParseFile(filePath: string) {
        return getFileExtension(filePath).toLowerCase() === ".json";
    }

    /** @inheritdoc */
    setGlobalConfiguration(globalConfig: ResolvedConfiguration) {
        this._resolveConfigurationResult = resolveConfiguration(globalConfig, this._unresolvedConfig);
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
        return parseJsonFile(jsonAst, fileText, this.getConfiguration());
    }

    /** @internal */
    private _getResolveConfigurationResult() {
        if (this._resolveConfigurationResult == null)
            return throwError("Global configuration must be set before calling this method.");
        return this._resolveConfigurationResult;
    }
}

import { Plugin, getFileExtension, ResolveConfigurationResult, ResolvedConfiguration, PrintItemIterable, ConfigurationDiagnostic,
    resolveConfiguration as resolveGlobalConfiguration } from "@dprint/core";
import { TypeScriptConfiguration, ResolvedTypeScriptConfiguration, resolveConfiguration } from "./configuration";
import { parseToBabelAst, parseTypeScriptFile } from "./parser";

export class TypeScriptPlugin implements Plugin<ResolvedTypeScriptConfiguration> {
    /** @internal */
    private readonly _unresolvedConfig: TypeScriptConfiguration;
    /** @internal */
    private _resolveConfigurationResult?: ResolveConfigurationResult<ResolvedTypeScriptConfiguration>;

    /**
     * Constructor.
     * @param config - The configuration to use.
     */
    constructor(config: TypeScriptConfiguration = {}) {
        this._unresolvedConfig = config;
    }

    /** @inheritdoc */
    version = "PACKAGE_VERSION"; // value is replaced at build time

    /** @inheritdoc */
    name = "dprint-plugin-typescript";

    /** @inheritdoc */
    shouldParseFile(filePath: string) {
        switch (getFileExtension(filePath).toLowerCase()) {
            case ".ts":
            case ".tsx":
            case ".js":
            case ".jsx": // todo: does jsx file path exist? I forget.
                return true;
            default:
                return false;
        }
    }

    /** @inheritdoc */
    setGlobalConfiguration(globalConfig: ResolvedConfiguration) {
        this._resolveConfigurationResult = resolveConfiguration(globalConfig, this._unresolvedConfig);
    }

    /** @inheritdoc */
    getConfiguration(): ResolvedTypeScriptConfiguration {
        return this._getResolveConfigurationResult().config;
    }

    /** @inheritdoc */
    getConfigurationDiagnostics(): ConfigurationDiagnostic[] {
        return this._getResolveConfigurationResult().diagnostics;
    }

    /** @inheritdoc */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false {
        const babelAst = parseToBabelAst(filePath, fileText);
        return parseTypeScriptFile(babelAst, fileText, this.getConfiguration());
    }

    /** @internal */
    private _getResolveConfigurationResult() {
        if (this._resolveConfigurationResult == null) {
            const globalConfig = resolveGlobalConfiguration({}).config;
            this._resolveConfigurationResult = resolveConfiguration(globalConfig, this._unresolvedConfig);
        }
        return this._resolveConfigurationResult;
    }
}

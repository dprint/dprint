import { Plugin, getFileExtension, ResolveConfigurationResult, ResolvedGlobalConfiguration, resolveGlobalConfiguration, PrintItemIterable } from "@dprint/core";
import { TypeScriptConfiguration, ResolvedTypeScriptConfiguration, resolveConfiguration } from "./configuration";
import { parseToBabelAst, parseTypeScriptFile } from "./parser";

export default class TypeScriptPlugin implements Plugin<TypeScriptConfiguration, ResolvedTypeScriptConfiguration> {
    /** @internal */
    private _config: ResolvedTypeScriptConfiguration | undefined;

    /** @internal */
    private get config() {
        if (this._config == null) {
            const defaultGlobalConfig = resolveGlobalConfiguration({}, []).config;
            this._config = resolveConfiguration(defaultGlobalConfig, {}).config;
        }

        return this._config;
    }

    /** @inheritdoc */
    version = "PACKAGE_VERSION"; // value is replaced at build time

    /** @inheritdoc */
    name = "dprint-plugin-typescript";

    /** @inheritdoc */
    configurationPropertyName = "typescript";

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
    setConfiguration(globalConfig: ResolvedGlobalConfiguration, pluginConfig: TypeScriptConfiguration): ResolveConfigurationResult<ResolvedTypeScriptConfiguration> {
        const result = resolveConfiguration(globalConfig, pluginConfig);
        this._config = result.config;
        return result;
    }

    /** @inheritdoc */
    getConfiguration(): ResolvedTypeScriptConfiguration {
        return this.config;
    }

    /** @inheritdoc */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false {
        const babelAst = parseToBabelAst(filePath, fileText);
        return parseTypeScriptFile(babelAst, fileText, this.config);
    }
}

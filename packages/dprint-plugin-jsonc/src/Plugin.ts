import { Plugin, getFileExtension, ResolveConfigurationResult, ResolvedGlobalConfiguration, resolveGlobalConfiguration, PrintItemIterable } from "@dprint/core";
import { JsoncConfiguration, ResolvedJsoncConfiguration, resolveConfiguration } from "./configuration";
import { parseToJsonAst, parseJsonFile } from "./parser";

export default class JsoncPlugin implements Plugin<JsoncConfiguration, ResolvedJsoncConfiguration> {
    /** @internal */
    private _config: ResolvedJsoncConfiguration | undefined;

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
    name = "dprint-plugin-jsonc";

    /** @inheritdoc */
    configurationPropertyName = "jsonc";

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
    setConfiguration(globalConfig: ResolvedGlobalConfiguration, pluginConfig: JsoncConfiguration): ResolveConfigurationResult<ResolvedJsoncConfiguration> {
        const result = resolveConfiguration(globalConfig, pluginConfig);
        this._config = result.config;
        return result;
    }

    /** @inheritdoc */
    getConfiguration(): ResolvedJsoncConfiguration {
        return this.config;
    }

    /** @inheritdoc */
    parseFile(filePath: string, fileText: string): PrintItemIterable | false {
        const babelAst = parseToJsonAst(fileText);
        return parseJsonFile(babelAst, fileText, this.config);
    }
}

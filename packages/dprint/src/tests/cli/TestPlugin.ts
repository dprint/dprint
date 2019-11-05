import { JsPlugin, BaseResolvedConfiguration, PluginInitializeOptions, PrintItemIterable, Signal } from "@dprint/types";
import { parserHelpers } from "@dprint/core";

export interface ResolvedTestPluginConfiguration extends BaseResolvedConfiguration {
}

export class TestPlugin implements JsPlugin<ResolvedTestPluginConfiguration> {
    version = "0.1.0";
    name = "dprint-plugin-test";

    shouldFormatFile(filePath: string, fileText: string): boolean {
        return filePath.endsWith(".ts");
    }

    initialize(options: PluginInitializeOptions): void {
    }

    getConfiguration(): ResolvedTestPluginConfiguration {
        return {
            indentWidth: 4,
            lineWidth: 80,
            newLineKind: "\n",
            useTabs: false
        };
    }

    getConfigurationDiagnostics() {
        return [];
    }

    *parseFile(filePath: string, fileText: string): PrintItemIterable {
        yield "// formatted";
        yield Signal.NewLine;
        yield* parserHelpers.parseRawString(fileText);
    }
}

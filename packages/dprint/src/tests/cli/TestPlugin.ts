import { Plugin, BaseResolvedConfiguration, PluginInitializeOptions, PrintItemIterable, PrintItemKind, Signal } from "@dprint/core";

export interface ResolvedTestPluginConfiguration extends BaseResolvedConfiguration {
}

export class TestPlugin implements Plugin<ResolvedTestPluginConfiguration> {
    version = "0.1.0";
    name = "dprint-plugin-test";

    shouldParseFile(filePath: string, fileText: string): boolean {
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
        yield {
            kind: PrintItemKind.RawString,
            text: fileText
        };
    }
}

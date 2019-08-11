import { Plugin, BaseResolvedConfiguration, ResolvedConfiguration, PrintItemIterable, PrintItemKind } from "@dprint/core";

export interface ResolvedTestPluginConfiguration extends BaseResolvedConfiguration {
}

export class TestPlugin implements Plugin<ResolvedTestPluginConfiguration> {
    version = "0.1.0";
    name = "dprint-plugin-test";

    shouldParseFile(filePath: string, fileText: string): boolean {
        return filePath.endsWith(".ts");
    }

    setGlobalConfiguration(globalConfig: ResolvedConfiguration): void {
    }

    getConfiguration(): ResolvedTestPluginConfiguration {
        return {
            indentWidth: 4,
            lineWidth: 80,
            newlineKind: "auto",
            useTabs: false
        };
    }

    getConfigurationDiagnostics() {
        return [];
    }

    *parseFile(filePath: string, fileText: string): PrintItemIterable {
        yield "// formatted";
        yield "\n";
        yield {
            kind: PrintItemKind.RawString,
            text: fileText
        };
    }
}

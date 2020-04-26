import { Plugin, BaseResolvedConfiguration, PluginInitializeOptions } from "@dprint/types";

export interface ResolvedTestPluginConfiguration extends BaseResolvedConfiguration {
}

export class TestPlugin implements Plugin<ResolvedTestPluginConfiguration> {
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
            newLineKind: "lf",
            useTabs: false,
        };
    }

    getConfigurationDiagnostics() {
        return [];
    }

    formatText(filePath: string, fileText: string) {
        return `// formatted\n${fileText}`;
    }
}

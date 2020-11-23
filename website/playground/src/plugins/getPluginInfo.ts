import { Language } from "../components";

export async function getPluginInfo(): Promise<PluginInfo[]> {
    const response = await fetch("https://plugins.dprint.dev/info.json");
    const json = await response.json();
    const expectedSchemaVersion = 2;

    if (json.schemaVersion !== expectedSchemaVersion) {
        throw new Error(`Expected schema version ${expectedSchemaVersion}, but found ${json.schemaVersion}.`);
    }

    const typescriptPlugin = json.latest.find((p: any) => p.configKey === "typescript")!;
    const jsonPlugin = json.latest.find((p: any) => p.configKey === "json")!;
    const markdownPlugin = json.latest.find((p: any) => p.configKey === "markdown")!;

    return [{
        url: typescriptPlugin.url,
        configSchemaUrl: "https://plugins.dprint.dev/schemas/typescript-v0.json",
        language: Language.TypeScript,
        fileExtensions: typescriptPlugin.fileExtensions,
    }, {
        url: jsonPlugin.url,
        configSchemaUrl: "https://plugins.dprint.dev/schemas/json-v0.json",
        language: Language.Json,
        fileExtensions: jsonPlugin.fileExtensions,
    }, {
        url: markdownPlugin.url,
        configSchemaUrl: "https://plugins.dprint.dev/schemas/markdown-v0.json",
        language: Language.Markdown,
        fileExtensions: markdownPlugin.fileExtensions,
    }];
}

export interface PluginInfo {
    url: string;
    configSchemaUrl: string;
    language: Language;
    fileExtensions: string[];
}

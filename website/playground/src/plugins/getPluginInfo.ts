export async function getPluginInfo(): Promise<PluginInfo[]> {
    const response = await fetch("https://plugins.dprint.dev/info.json");
    const json = await response.json();
    const expectedSchemaVersion = 1;

    if (json.schemaVersion !== expectedSchemaVersion) {
        throw new Error(`Expected schema version ${expectedSchemaVersion}, but found ${json.schemaVersion}.`);
    }

    const typescriptPlugin = json.latest.find((p: any) => p.configKey === "typescript")!;
    const jsonPlugin = json.latest.find((p: any) => p.configKey === "json")!;

    return [{
        url: typescriptPlugin.url,
        configSchemaUrl: "https://plugins.dprint.dev/schemas/typescript-v0.json",
        language: "typescript",
        fileExtensions: typescriptPlugin.fileExtensions,
    }, {
        url: jsonPlugin.url,
        configSchemaUrl: "https://plugins.dprint.dev/schemas/json-v0.json",
        language: "json",
        fileExtensions: jsonPlugin.fileExtensions,
    }];
}

export interface PluginInfo {
    url: string;
    configSchemaUrl: string;
    language: "typescript" | "json";
    fileExtensions: string[];
}

export async function getPluginUrls(signal: AbortSignal): Promise<string[]> {
  const response = await fetch("https://plugins.dprint.dev/info.json", { signal });
  const json = await response.json();
  const expectedSchemaVersion = 4;

  if (json.schemaVersion !== expectedSchemaVersion) {
    throw new Error(`Expected schema version ${expectedSchemaVersion}, but found ${json.schemaVersion}.`);
  }

  const typescriptPlugin = json.latest.find((p: any) => p.configKey === "typescript")!;
  const jsonPlugin = json.latest.find((p: any) => p.configKey === "json")!;
  const markdownPlugin = json.latest.find((p: any) => p.configKey === "markdown")!;
  const tomlPlugin = json.latest.find((p: any) => p.configKey === "toml")!;
  const dockerfilePlugin = json.latest.find((p: any) => p.configKey === "dockerfile")!;

  return [typescriptPlugin.url, jsonPlugin.url, markdownPlugin.url, tomlPlugin.url, dockerfilePlugin.url];
}

export function getLanguageFromPluginUrl(url: string) {
  const result = /https:\/\/plugins\.dprint\.dev\/([a-z]+)-[0-9]+\.[0-9]+\.[0-9]+\.wasm$/.exec(url);
  const language = result?.[1];
  switch (language) {
    case "typescript":
    case "markdown":
    case "json":
    case "toml":
    case "dockerfile":
      return language;
    default:
      return undefined;
  }
}

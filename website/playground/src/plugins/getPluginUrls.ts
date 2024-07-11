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
  const biomePlugin = json.latest.find((p: any) => p.configKey === "biome")!;
  const ruffPlugin = json.latest.find((p: any) => p.configKey === "ruff")!;
  const malvaPlugin = json.latest.find((p: any) => p.configKey === "malva")!;
  const markupFmtPlugin = json.latest.find((p: any) => p.configKey === "markup")!;
  const prettyYamlPlugin = json.latest.find((p: any) => p.configKey === "yaml")!;

  return [typescriptPlugin.url, jsonPlugin.url, markdownPlugin.url, tomlPlugin.url, dockerfilePlugin.url, biomePlugin.url, ruffPlugin.url, malvaPlugin.url, markupFmtPlugin.url, prettyYamlPlugin.url];
}

export function getPluginShortNameFromPluginUrl(url: string) {
  const result = /https:\/\/plugins\.dprint\.dev\/([a-z/_-]+)-v?[0-9]+\.[0-9]+\.[0-9]+\.wasm$/.exec(url);
  const name = result?.[1];
  switch (name) {
    case "typescript":
    case "markdown":
    case "json":
    case "toml":
    case "dockerfile":
    case "biome":
    case "ruff":
      return name;
    case "g-plane/malva":
    case "g-plane/markup_fmt":
    case "g-plane/pretty_yaml":
      // user name is removed because there can't be `/` in the url
      return name.split("/")[1];
    default:
      return undefined;
  }
}

export function getLanguageFromPluginUrl(url: string) {
  const result = /https:\/\/plugins\.dprint\.dev\/([a-z/_-]+)-v?[0-9]+\.[0-9]+\.[0-9]+\.wasm$/.exec(url);
  const language = result?.[1];
  switch (language) {
    case "typescript":
    case "markdown":
    case "json":
    case "toml":
    case "dockerfile":
      return language;
    case "biome":
      return "typescript";
    case "ruff":
      // todo: specify python here eventually (probably need to upgrade the code editor)
      return "plaintext";
    case "g-plane/malva":
      return "css";
    case "g-plane/markup_fmt":
      return "html";
    case "g-plane/pretty_yaml":
      return "yaml";
    default:
      return undefined;
  }
}

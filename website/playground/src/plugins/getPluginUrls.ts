export async function getPluginUrls(signal: AbortSignal): Promise<string[]> {
  const response = await fetch("https://plugins.dprint.dev/info.json", { signal });
  const json = await response.json();
  const expectedSchemaVersion = 4;

  if (json.schemaVersion !== expectedSchemaVersion) {
    throw new Error(`Expected schema version ${expectedSchemaVersion}, but found ${json.schemaVersion}.`);
  }

  // every Wasm plugin, since process plugins can't run in the browser
  return json.latest
    .map((p: any) => p.url as string)
    .filter((url: string) => url.endsWith(".wasm"));
}

const RE_PLUGIN_URL = /https:\/\/plugins\.dprint\.dev\/(?:[a-z_-]+\/)?([a-z_-]+)-v?[0-9]+\.[0-9]+\.[0-9]+\.wasm$/;

export function getPluginShortNameFromPluginUrl(url: string) {
  const result = RE_PLUGIN_URL.exec(url);
  const name = result?.[1];
  switch (name) {
    case "typescript":
    case "markdown":
    case "json":
    case "toml":
    case "dockerfile":
    case "biome":
    case "oxc":
    case "bun":
    case "mago":
    case "ruff":
    case "roslyn":
    case "malva":
    case "markup_fmt":
    case "pretty_yaml":
    case "pretty_graphql":
      return name;
    default:
      return undefined;
  }
}

// the name to show for a plugin, which unlike its short name isn't
// limited to the plugins that have a short name in shared urls
export function getPluginDisplayNameFromPluginUrl(url: string) {
  return RE_PLUGIN_URL.exec(url)?.[1];
}

export function getLanguageFromPluginUrl(url: string) {
  const result = RE_PLUGIN_URL.exec(url);
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
    case "oxc":
      return "typescript";
    case "bun":
      return "typescript";
    case "mago":
      return "php";
    case "ruff":
      // todo: specify python here eventually (probably need to upgrade the code editor)
      return "plaintext";
    case "roslyn":
      return "csharp";
    case "malva":
    case "lax-css":
      return "css";
    case "markup_fmt":
    case "lax-markup":
      return "html";
    case "pretty_yaml":
      return "yaml";
    default:
      return undefined;
  }
}

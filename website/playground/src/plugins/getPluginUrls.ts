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
    case "csharpier":
    case "clang-format":
    case "cedar":
    case "sql":
    case "typstyle":
    case "nix":
    case "alejandra":
    case "kdl":
    case "shuck":
    case "shfmt":
    case "stylua":
    case "zig":
    case "terraform":
    case "buildifier":
    case "wat":
    case "jupyter":
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
    case "csharpier":
      return "csharp";
    case "clang-format":
      return "cpp";
    case "cedar":
      return "plaintext";
    case "sql":
      return "sql";
    case "shuck":
    case "shfmt":
      return "shell";
    case "stylua":
      return "lua";
    case "terraform":
      return "hcl";
    case "buildifier":
      // starlark is a dialect of python
      return "python";
    case "typstyle":
    case "nix":
    case "alejandra":
    case "kdl":
    case "zig":
    case "wat":
      return "plaintext";
    case "jupyter":
      return "json";
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

// the plugins that format the code a plugin embeds, since a plugin like jupyter
// only formats its code with other plugins
const hostPluginNames: Record<string, string[]> = {
  jupyter: ["markdown", "ruff", "typescript"],
};

/** Gets the urls of the plugins that format the code the plugin at the url embeds. */
export function getHostPluginUrls(pluginUrl: string, pluginUrls: string[]) {
  const names = hostPluginNames[getPluginShortNameFromPluginUrl(pluginUrl) ?? ""] ?? [];
  return names
    .map(name => pluginUrls.find(url => getPluginShortNameFromPluginUrl(url) === name))
    .filter((url): url is string => url != null);
}

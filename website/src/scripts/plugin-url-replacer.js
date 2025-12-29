// Replaces plugin links with the latest version.
const pluginInfoUrl = "https://plugins.dprint.dev/info.json";
const schemaVersion = 4;

// Pre-compute quoted placeholder URLs at module load time
const pluginPlaceholders = new Map([
  ["\"https://plugins.dprint.dev/typescript-x.x.x.wasm\"", "dprint-plugin-typescript"],
  ["\"https://plugins.dprint.dev/json-x.x.x.wasm\"", "dprint-plugin-json"],
  ["\"https://plugins.dprint.dev/markdown-x.x.x.wasm\"", "dprint-plugin-markdown"],
  ["\"https://plugins.dprint.dev/toml-x.x.x.wasm\"", "dprint-plugin-toml"],
  ["\"https://plugins.dprint.dev/dockerfile-x.x.x.wasm\"", "dprint-plugin-dockerfile"],
  ["\"https://plugins.dprint.dev/biome-x.x.x.wasm\"", "dprint-plugin-biome"],
  ["\"https://plugins.dprint.dev/oxc-x.x.x.wasm\"", "dprint-plugin-oxc"],
  ["\"https://plugins.dprint.dev/ruff-x.x.x.wasm\"", "dprint-plugin-ruff"],
  ["\"https://plugins.dprint.dev/jupyter-x.x.x.wasm\"", "dprint-plugin-jupyter"],
  ["\"https://plugins.dprint.dev/g-plane/malva-vx.x.x.wasm\"", "g-plane/malva"],
  ["\"https://plugins.dprint.dev/g-plane/markup_fmt-vx.x.x.wasm\"", "g-plane/markup_fmt"],
  ["\"https://plugins.dprint.dev/g-plane/pretty_yaml-vx.x.x.wasm\"", "g-plane/pretty_yaml"],
  ["\"https://plugins.dprint.dev/g-plane/pretty_graphql-vx.x.x.wasm\"", "g-plane/pretty_graphql"],
]);

export function replacePluginUrls() {
  const elements = getPluginUrlElements();
  if (elements.length > 0) {
    getPluginInfo().then((pluginUrls) => {
      for (const element of getPluginUrlElements()) {
        const pluginName = pluginPlaceholders.get(element.textContent);
        const url = pluginUrls.get(pluginName);
        if (url != null) {
          element.textContent = "\"" + url + "\"";
        }
      }
    });
  }
}

function getPluginUrlElements() {
  const stringElements = document.getElementsByClassName("hljs-string");
  const result = [];
  for (let i = 0; i < stringElements.length; i++) {
    const stringElement = stringElements.item(i);
    if (pluginPlaceholders.has(stringElement.textContent)) {
      result.push(stringElement);
    }
  }
  return result;
}

function getPluginInfo() {
  return fetch(pluginInfoUrl)
    .then((response) => response.json())
    .then((data) => {
      if (data.schemaVersion !== schemaVersion) {
        throw new Error("Expected schema version " + schemaVersion + ", but found " + data.schemaVersion);
      }

      const result = new Map();
      for (const pluginInfo of data.latest) {
        result.set(pluginInfo.name, pluginInfo.url);
      }
      return result;
    });
}

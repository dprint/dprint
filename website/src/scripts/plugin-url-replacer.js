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
  ["\"https://plugins.dprint.dev/jakebailey/gofumpt-vx.x.x.wasm\"", "jakebailey/dprint-plugin-gofumpt"],
  ["\"https://plugins.dprint.dev/jolars/panache-x.x.x.wasm\"", "jolars/panache"],
  ["\"https://plugins.dprint.dev/jolars/badness-vx.x.x.wasm\"", "jolars/badness"],
  ["\"https://plugins.dprint.dev/jolars/arity-vx.x.x.wasm\"", "jolars/arity"],
  ["\"https://plugins.dprint.dev/jolars/fatou-vx.x.x.wasm\"", "jolars/fatou"],
  ["\"https://plugins.dprint.dev/apcamargo/typstyle-x.x.x.wasm\"", "apcamargo/typstyle"],
  ["\"https://plugins.dprint.dev/apcamargo/bibtex-tidy-x.x.x.wasm\"", "apcamargo/bibtex-tidy"],
  ["\"https://plugins.dprint.dev/kachick/typstyle-x.x.x.wasm\"", "kachick/typstyle"],
  ["\"https://plugins.dprint.dev/kachick/nix-x.x.x.wasm\"", "kachick/nix"],
  ["\"https://plugins.dprint.dev/kachick/kdl-x.x.x.wasm\"", "kachick/kdl"],
  ["\"https://plugins.dprint.dev/kachick/sh-x.x.x.wasm\"", "kachick/sh"],
  ["\"https://plugins.dprint.dev/bartlomieju/lax-css-x.x.x.wasm\"", "bartlomieju/lax-css"],
  ["\"https://plugins.dprint.dev/bartlomieju/lax-markup-x.x.x.wasm\"", "bartlomieju/lax-markup"],
  ["\"https://plugins.dprint.dev/bartlomieju/lax-sql-x.x.x.wasm\"", "bartlomieju/lax-sql"],
  ["\"https://plugins.dprint.dev/sargunv/dprint-clang-format-x.x.x.wasm\"", "sargunv/dprint-clang-format"],
  ["\"https://plugins.dprint.dev/sargunv/dprint-cmakefmt-x.x.x.wasm\"", "sargunv/dprint-cmakefmt"],
]);

export function replacePluginUrls() {
  const elements = getPluginUrlElements();
  if (elements.length > 0) {
    getPluginInfo().then((pluginUrls) => {
      for (const element of elements) {
        const pluginName = pluginPlaceholders.get(element.textContent);
        const url = pluginUrls.get(pluginName);
        if (url != null) {
          element.textContent = "\"" + url + "\"";
        } else if (pluginName != null) {
          // some plugins (ex. jakebailey/gofumpt) aren't listed in info.json, so
          // fall back to the plugin's own latest.json to resolve the newest url
          getLatestPluginUrl(pluginName).then((fallbackUrl) => {
            if (fallbackUrl != null) {
              element.textContent = "\"" + fallbackUrl + "\"";
            }
          });
        }
      }
    });
  }
}

function getLatestPluginUrl(pluginName) {
  return fetch("https://plugins.dprint.dev/" + pluginName + "/latest.json")
    .then((response) => (response.ok ? response.json() : null))
    .then((data) => (data == null ? null : data.url))
    .catch(() => null);
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

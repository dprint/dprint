// Replaces plugin links with the latest version.
const pluginInfoUrl = "https://plugins.dprint.dev/info.json";
const schemaVersion = 4;

// placeholder npm specifiers (ex. `"npm:@dprint/typescript@x.x.x"`)
const npmPlaceholderRe = /^"npm:(.+)@x\.x\.x"$/;

// quoted placeholder URLs of the plugins that aren't distributed on npm
const pluginPlaceholders = new Map([
  ["\"https://plugins.dprint.dev/jolars/panache-x.x.x.wasm\"", "jolars/panache"],
  ["\"https://plugins.dprint.dev/jolars/badness-vx.x.x.wasm\"", "jolars/badness"],
  ["\"https://plugins.dprint.dev/jolars/arity-vx.x.x.wasm\"", "jolars/arity"],
  ["\"https://plugins.dprint.dev/jolars/fatou-vx.x.x.wasm\"", "jolars/fatou"],
  ["\"https://plugins.dprint.dev/apcamargo/typstyle-x.x.x.wasm\"", "apcamargo/typstyle"],
  ["\"https://plugins.dprint.dev/apcamargo/bibtex-tidy-x.x.x.wasm\"", "apcamargo/bibtex-tidy"],
  ["\"https://plugins.dprint.dev/sargunv/dprint-clang-format-x.x.x.wasm\"", "sargunv/dprint-clang-format"],
  ["\"https://plugins.dprint.dev/sargunv/dprint-cmakefmt-x.x.x.wasm\"", "sargunv/dprint-cmakefmt"],
]);

export function replacePluginUrls() {
  const elements = getPluginUrlElements();
  if (elements.length > 0) {
    getPluginInfo().then(({ pluginUrls, npmVersions }) => {
      for (const element of elements) {
        const npmName = npmPlaceholderRe.exec(element.textContent)?.[1];
        if (npmName != null) {
          const version = npmVersions.get(npmName);
          if (version != null) {
            element.textContent = "\"npm:" + npmName + "@" + version + "\"";
          }
          continue;
        }
        const pluginName = pluginPlaceholders.get(element.textContent);
        const url = pluginUrls.get(pluginName);
        if (url != null) {
          element.textContent = "\"" + url + "\"";
        } else if (pluginName != null) {
          // some plugins aren't listed in info.json, so
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
    if (pluginPlaceholders.has(stringElement.textContent) || npmPlaceholderRe.test(stringElement.textContent)) {
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

      const pluginUrls = new Map();
      const npmVersions = new Map();
      for (const pluginInfo of data.latest) {
        pluginUrls.set(pluginInfo.name, pluginInfo.url);
        if (pluginInfo.npm != null) {
          npmVersions.set(pluginInfo.npm.name, pluginInfo.npm.version);
        }
      }
      return { pluginUrls, npmVersions };
    });
}

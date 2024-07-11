import { compressToEncodedURIComponent, decompressFromEncodedURIComponent } from "lz-string";

export const knownPlugins = new Set([
  "typescript",
  "json",
  "markdown",
  "toml",
  "dockerfile",
  "biome",
  "ruff",
  // third party plugins, but user name is removed because there can't be `/` in the url
  "malva",
  "markup_fmt",
  "pretty_yaml",
]);

export class UrlSaver {
  getUrlInfo() {
    const locationHash = document.location.hash || "";

    return {
      text: getText(),
      configText: getConfigText(),
      ...getPlugin(),
    };

    function getText() {
      const matches = /code\/([^/]+)/.exec(locationHash);
      if (matches == null || matches.length !== 2) {
        return "";
      }

      try {
        return decompress(matches[1]);
      } catch (err) {
        console.error(err);
        return "";
      }
    }

    function getConfigText(): string | undefined {
      const matches = /config\/([^/]+)/.exec(locationHash);
      if (matches == null || matches.length !== 2) {
        return undefined;
      }

      try {
        return decompress(matches[1]);
      } catch (err) {
        console.error(err);
        return undefined;
      }
    }

    function getPlugin(): { pluginName?: string; pluginUrl?: string } {
      const matches = /plugin\/([^/]+)/.exec(locationHash);
      if (matches == null || matches.length !== 2) {
        return {
          pluginName: getLegacyLanguage(),
          pluginUrl: undefined,
        };
      }

      if (knownPlugins.has(matches[1])) {
        return {
          pluginName: matches[1] as string,
          pluginUrl: undefined,
        };
      }

      try {
        return {
          pluginName: undefined,
          pluginUrl: decompress(matches[1]),
        };
      } catch (err) {
        console.error(err);
        return {};
      }
    }

    function getLegacyLanguage(): string {
      const matches = /language\/([^/]+)/.exec(locationHash);
      if (matches == null || matches.length !== 2) {
        return "typescript";
      }

      try {
        if (knownPlugins.has(matches[1])) {
          return matches[1] as string;
        } else {
          return "typescript";
        }
      } catch (err) {
        console.error(err);
        return "typescript";
      }
    }
  }

  updateUrl({ text, configText, plugin }: {
    text: string;
    configText?: string;
    plugin?: string;
  }) {
    let url = `#code/${compressToEncodedURIComponent(text)}`;
    if (configText != null) {
      url += `/config/${compressToEncodedURIComponent(configText)}`;
    }
    if (plugin != null) {
      url += `/plugin/${knownPlugins.has(plugin) ? plugin : compressToEncodedURIComponent(plugin)}`;
    }
    window.history.replaceState(
      undefined,
      "",
      url,
    );
  }
}

function decompress(text: string) {
  return decompressFromEncodedURIComponent(text.trim()) || ""; // will be null on error
}

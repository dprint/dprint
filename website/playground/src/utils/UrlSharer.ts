import { compressToEncodedURIComponent, decompressFromEncodedURIComponent } from "lz-string";

export class UrlSaver {
  getUrlInfo() {
    const locationHash = document.location.hash || "";

    return {
      text: getText(),
      configText: getConfigText(),
      plugin: getPluginUrl(),
      language: getLanguage(),
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

    function getPluginUrl(): string | undefined {
      const matches = /plugin\/([^/]+)/.exec(locationHash);
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

    function getLanguage(): "typescript" | "json" | "markdown" | "toml" | "dockerfile" {
      const matches = /language\/([^/]+)/.exec(locationHash);
      if (matches == null || matches.length !== 2) {
        return "typescript";
      }

      try {
        switch (matches[1]) {
          case "json":
            return "json";
          case "markdown":
            return "markdown";
          case "toml":
            return "toml";
          case "dockerfile":
            return "dockerfile";
          case "typescript":
          default:
            return "typescript";
        }
      } catch (err) {
        console.error(err);
        return "typescript";
      }
    }
  }

  updateUrl({ text, configText, plugin, language }: {
    text: string;
    configText?: string;
    plugin?: string;
    language?: "typescript" | "json" | "markdown" | "toml" | "dockerfile";
  }) {
    let url = `#code/${compressToEncodedURIComponent(text)}`;
    if (configText != null) {
      url += `/config/${compressToEncodedURIComponent(configText)}`;
    }
    if (plugin != null) {
      url += `/plugin/${compressToEncodedURIComponent(plugin)}`;
    }
    if (language != null) {
      url += `/language/${language}`;
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

import { TypeScriptConfiguration } from "dprint-plugin-typescript";
import { decompressFromEncodedURIComponent, compressToEncodedURIComponent } from "lz-string";

export class UrlSaver {
    getUrlInfo() {
        const locationHash = document.location.hash || "";

        return {
            text: getText(),
            config: getConfig()
        };

        function getText() {
            const matches = /code\/([^/]+)/.exec(locationHash);
            if (matches == null || matches.length !== 2)
                return "";

            try {
                return decompress(matches[1]);
            } catch (err) {
                console.error(err);
                return "";
            }
        }

        function getConfig(): TypeScriptConfiguration {
            const matches = /config\/([^/]+)/.exec(locationHash);
            if (matches == null || matches.length !== 2)
                return getDefaultConfig();

            try {
                return JSON.parse(decompress(matches[1]));
            } catch (err) {
                console.error(err);
                return getDefaultConfig();
            }

            function getDefaultConfig(): TypeScriptConfiguration {
                return {
                    lineWidth: 80
                };
            }
        }
    }

    updateUrl({ text, config }: { text: string; config: TypeScriptConfiguration; }) {
        window.history.replaceState(
            undefined,
            "",
            `#code/${compressToEncodedURIComponent(text)}/config/${compressToEncodedURIComponent(JSON.stringify(config))}`
        );
    }
}

function decompress(text: string) {
    return decompressFromEncodedURIComponent(text.trim()) || ""; // will be null on error
}

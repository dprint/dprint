// Replaces plugin links with the latest version.
(function(Dprint) {
    var typescriptUrl = "https://plugins.dprint.dev/typescript-x.x.x.wasm";
    var jsonUrl = "https://plugins.dprint.dev/json-x.x.x.wasm";
    var markdownUrl = "https://plugins.dprint.dev/markdown-x.x.x.wasm";
    var rustFmtUrl = "https://plugins.dprint.dev/rustfmt-x.x.x.wasm";
    var pluginInfoUrl = "https://plugins.dprint.dev/info.json";
    var schemaVersion = 1;

    Dprint.replacePluginUrls = function() {
        var elements = getPluginUrlElements();
        if (elements.length > 0) {
            getPluginInfo().then(function(urls) {
                for (let i = 0; i < elements.length; i++) {
                    var element = elements[i];
                    switch (element.textContent) {
                        case getWithQuotes(typescriptUrl):
                            element.textContent = getWithQuotes(urls["typescript"]);
                            break;
                        case getWithQuotes(jsonUrl):
                            element.textContent = getWithQuotes(urls["json"]);
                            break;
                        case getWithQuotes(markdownUrl):
                            element.textContent = getWithQuotes(urls["markdown"]);
                            break;
                        case getWithQuotes(rustFmtUrl):
                            element.textContent = getWithQuotes(urls["rustfmt"]);
                            break;
                    }
                }
            });
        }
    };

    function getPluginUrlElements() {
        var stringElements = document.getElementsByClassName("hljs-string");
        var result = [];
        for (var i = 0; i < stringElements.length; i++) {
            var stringElement = stringElements.item(i);
            switch (stringElement.textContent) {
                case getWithQuotes(typescriptUrl):
                case getWithQuotes(jsonUrl):
                case getWithQuotes(markdownUrl):
                case getWithQuotes(rustFmtUrl):
                    result.push(stringElement);
                    break;
            }
        }
        return result;
    }

    function getWithQuotes(text) {
        return "\"" + text + "\"";
    }

    function getPluginInfo() {
        return fetch(pluginInfoUrl)
            .then(function(response) {
                return response.json();
            })
            .then(function(data) {
                if (data.schemaVersion !== schemaVersion) {
                    throw new Error("Expected schema version " + schemaVersion + ", but found " + data.schemaVersion);
                }

                return {
                    typescript: getUrlForPlugin(data, "dprint-plugin-typescript"),
                    json: getUrlForPlugin(data, "dprint-plugin-json"),
                    markdown: getUrlForPlugin(data, "dprint-plugin-markdown"),
                    rustfmt: getUrlForPlugin(data, "dprint-plugin-rustfmt"),
                };
            });

        function getUrlForPlugin(data, pluginName) {
            var pluginInfo = data.latest.find(function(pluginInfo) {
                return pluginInfo.name === pluginName;
            });
            if (pluginInfo == null) {
                throw new Error("Could not find plugin with name " + pluginName);
            }

            return pluginInfo.url;
        }
    }
})(window.Dprint || (window.Dprint = {}));

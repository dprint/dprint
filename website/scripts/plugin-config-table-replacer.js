// generates the plugin config table
(function(Dprint) {
    Dprint.replaceConfigTable = function() {
        var items = getPluginConfigTableItems();
        if (items.length > 0) {
            items.forEach(function(item) {
                getDprintPluginConfig(item.url).then(function(properties) {
                    var element = item.element;
                    element.innerHTML = "<p>This information was auto generated from <a href=\"" + item.url + "\">" + item.url + "</a>.</p>";
                    properties.forEach(function(property) {
                        var propertyContainer = document.createElement("div");
                        element.appendChild(propertyContainer);
                        // title
                        var propertyTitle = document.createElement("h2");
                        if (property.name === "preferSingleLine") {
                            property.name += " (Very Experimental)";
                        }
                        propertyTitle.textContent = property.name;
                        propertyContainer.appendChild(propertyTitle);

                        // description
                        var propertyDesc = document.createElement("p");
                        propertyDesc.textContent = property.description;
                        propertyContainer.appendChild(propertyDesc);

                        var infoContainer = document.createElement("ul");
                        propertyContainer.appendChild(infoContainer);

                        if (property.oneOf) {
                            property.oneOf.forEach(function(oneOf) {
                                var oneOfContainer = document.createElement("li");
                                infoContainer.appendChild(oneOfContainer);
                                var prefix = document.createElement("strong");
                                prefix.textContent = valueToText(oneOf.const);
                                oneOfContainer.appendChild(prefix);
                                if (oneOf.description != null && oneOf.description.length > 0) {
                                    oneOfContainer.append(" - " + oneOf.description);
                                }
                                if (oneOf.const === property.default) {
                                    oneOfContainer.append(" (Default)");
                                }
                            });
                        } else {
                            // type
                            var typeContainer = document.createElement("li");
                            infoContainer.appendChild(typeContainer);
                            var typePrefix = document.createElement("strong");
                            typePrefix.textContent = "Type: ";
                            typeContainer.appendChild(typePrefix);
                            typeContainer.append(property.type);

                            // default
                            var defaultContainer = document.createElement("li");
                            infoContainer.appendChild(defaultContainer);
                            var defaultPrefix = document.createElement("strong");
                            defaultPrefix.textContent = "Default: ";
                            defaultContainer.appendChild(defaultPrefix);
                            defaultContainer.append(valueToText(property.default));
                        }

                        if (property.astSpecificProperties != null && property.astSpecificProperties.length > 0) {
                            var astSpecificPropertiesPrefix = document.createElement("p");
                            astSpecificPropertiesPrefix.textContent = "AST node specific configuration property names:";
                            propertyContainer.appendChild(astSpecificPropertiesPrefix);

                            var astSpecificPropertyNamesContainer = document.createElement("ul");
                            propertyContainer.appendChild(astSpecificPropertyNamesContainer);

                            property.astSpecificProperties.forEach(function(propName) {
                                var propertyNameLi = document.createElement("li");
                                propertyNameLi.textContent = valueToText(propName);
                                astSpecificPropertyNamesContainer.appendChild(propertyNameLi);
                            });
                        }
                    });

                    function valueToText(value) {
                        if (typeof value === "string") {
                            return "\"" + value + "\"";
                        }
                        return value.toString();
                    }
                });
            });
        }
    };

    function getPluginConfigTableItems() {
        var result = [];
        var elements = document.getElementsByClassName("plugin-config-table");
        for (var i = 0; i < elements.length; i++) {
            var element = elements.item(i);
            result.push({
                element,
                url: element.dataset.url,
            });
        }
        return result;
    }

    function getDprintPluginConfig(configSchemaUrl) {
        return fetch(configSchemaUrl).then(function(response) {
            return response.json();
        }).then(function(json) {
            var properties = {};
            var order = 0;

            for (const propertyName of Object.keys(json.properties)) {
                if (propertyName === "$schema" || propertyName === "deno" || propertyName === "locked") {
                    continue;
                }
                var property = json.properties[propertyName];

                if (property["$ref"]) {
                    var definition = json.definitions[propertyName];
                    if (definition != null) {
                        ensurePropertyName(propertyName);
                        properties[propertyName] = Object.assign(properties[propertyName], definition);
                        properties[propertyName].order = order++;
                        properties[propertyName].name = propertyName;
                    } else {
                        var derivedPropName = property["$ref"].replace("#/definitions/", "");
                        ensurePropertyName(derivedPropName);
                        properties[derivedPropName].astSpecificProperties.push(propertyName);
                    }
                } else {
                    ensurePropertyName(propertyName);
                    properties[propertyName] = Object.assign(properties[propertyName], property);
                    properties[propertyName].order = order++;
                    properties[propertyName].name = propertyName;
                }
            }

            var propertyArray = [];
            var propertyKeys = Object.keys(properties);
            for (var i = 0; i < propertyKeys.length; i++) {
                var propName = propertyKeys[i];
                propertyArray.push(properties[propName]);
            }
            propertyArray.sort(function(a, b) {
                return a.order - b.order;
            });
            return propertyArray;

            function ensurePropertyName(propertyName) {
                if (properties[propertyName] == null) {
                    properties[propertyName] = {
                        astSpecificProperties: [],
                    };
                }
            }
        });
    }
})(window.Dprint || (window.Dprint = {}));

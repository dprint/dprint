// generates the plugin config table
export function replaceConfigTable() {
  const items = getPluginConfigTableItems();
  if (items.length > 0) {
    items.forEach(function(item) {
      getDprintPluginConfig(item.url).then((properties) => {
        const element = item.element;
        element.innerHTML = "<p>This information was auto generated from <a href=\"" + item.url + "\">" + item.url + "</a>.</p>";
        properties.forEach(function(property) {
          const propertyContainer = document.createElement("div");
          element.appendChild(propertyContainer);
          try {
            // title
            const propertyTitle = document.createElement("h2");
            if (property.name === "preferSingleLine") {
              property.name += " (Very Experimental)";
            }
            propertyTitle.textContent = property.name;
            propertyContainer.appendChild(propertyTitle);

            addDescription(propertyContainer, property);
            addInfoContainer(propertyContainer, property);

            if (property.astSpecificProperties != null && property.astSpecificProperties.length > 0) {
              const astSpecificPropertiesPrefix = document.createElement("p");
              astSpecificPropertiesPrefix.textContent = "AST node specific configuration property names:";
              propertyContainer.appendChild(astSpecificPropertiesPrefix);

              const astSpecificPropertyNamesContainer = document.createElement("ul");
              propertyContainer.appendChild(astSpecificPropertyNamesContainer);

              property.astSpecificProperties.forEach(function({ propertyName, definition }) {
                const propertyNameLi = document.createElement("li");

                const labelSpan = document.createElement("span");
                labelSpan.textContent = valueToText(propertyName);
                propertyNameLi.appendChild(labelSpan);

                if (definition != null) {
                  const definitionDiv = document.createElement("div");
                  if (definition.description !== property.description) {
                    addDescription(definitionDiv, definition);
                  }
                  addInfoContainer(definitionDiv, definition);
                  propertyNameLi.appendChild(definitionDiv);
                }

                astSpecificPropertyNamesContainer.appendChild(propertyNameLi);
              });
            }
          } catch (err) {
            console.error(err);
            const errorMessage = document.createElement("strong");
            errorMessage.textContent = "Error getting property information. Check the browser console.";
            errorMessage.style.color = "red";
            propertyContainer.appendChild(errorMessage);
          }
        });

        function addDescription(propertyContainer, property) {
          const propertyDesc = document.createElement("p");
          propertyDesc.textContent = property.description;
          propertyContainer.appendChild(propertyDesc);
        }

        function addInfoContainer(propertyContainer, property) {
          const infoContainer = document.createElement("ul");
          propertyContainer.appendChild(infoContainer);

          if (property.oneOf) {
            property.oneOf.forEach(function(oneOf) {
              const oneOfContainer = document.createElement("li");
              infoContainer.appendChild(oneOfContainer);
              const prefix = document.createElement("strong");
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
            const typeContainer = document.createElement("li");
            infoContainer.appendChild(typeContainer);
            const typePrefix = document.createElement("strong");
            typePrefix.textContent = "Type: ";
            typeContainer.appendChild(typePrefix);
            typeContainer.append(property.type);

            // default
            const defaultContainer = document.createElement("li");
            infoContainer.appendChild(defaultContainer);
            const defaultPrefix = document.createElement("strong");
            defaultPrefix.textContent = "Default: ";
            defaultContainer.appendChild(defaultPrefix);
            defaultContainer.append(valueToText(property.default));
          }
        }

        function valueToText(value) {
          if (typeof value === "string") {
            return "\"" + value + "\"";
          }
          if (value == null) {
            return "<not specified>";
          }
          return value.toString();
        }
      });
    });
  }
}

function getPluginConfigTableItems() {
  const result = [];
  const elements = document.getElementsByClassName("plugin-config-table");
  for (let i = 0; i < elements.length; i++) {
    const element = elements.item(i);
    result.push({
      element,
      url: element.dataset.url,
    });
  }
  return result;
}

function getDprintPluginConfig(configSchemaUrl) {
  return fetch(configSchemaUrl).then((response) => {
    return response.json();
  }).then((json) => {
    const properties = {};
    let order = 0;

    for (const propertyName of Object.keys(json.properties)) {
      if (propertyName === "$schema" || propertyName === "deno" || propertyName === "locked") {
        continue;
      }
      const property = json.properties[propertyName];

      if (property["$ref"]) {
        const derivedPropName = property["$ref"].replace("#/definitions/", "");

        const lastSegment = propertyName.split(".").pop();
        let parentProperty;
        if (derivedPropName !== propertyName && derivedPropName in json.properties) {
          parentProperty = derivedPropName;
        } else if (lastSegment !== propertyName && lastSegment in json.properties) {
          parentProperty = lastSegment;
        }

        const definition = json.definitions[derivedPropName];
        if (parentProperty) {
          ensurePropertyName(parentProperty);
          const isSameDefinition = property["$ref"] === json.properties[parentProperty]["$ref"];
          properties[parentProperty].astSpecificProperties.push({
            propertyName,
            definition: isSameDefinition ? null : definition,
          });
        } else {
          setDefinitionForPropertyName(propertyName, definition);
        }
      } else {
        ensurePropertyName(propertyName);
        properties[propertyName] = Object.assign(properties[propertyName], property);
        properties[propertyName].order = order++;
        properties[propertyName].name = propertyName;
      }
    }

    const propertyArray = [];
    const propertyKeys = Object.keys(properties);
    for (let i = 0; i < propertyKeys.length; i++) {
      const propName = propertyKeys[i];
      propertyArray.push(properties[propName]);
    }
    propertyArray.sort((a, b) => a.order - b.order);
    return propertyArray;

    function setDefinitionForPropertyName(propertyName, definition) {
      ensurePropertyName(propertyName);
      properties[propertyName] = Object.assign(properties[propertyName], definition);
      properties[propertyName].order = order++;
      properties[propertyName].name = propertyName;
    }

    function ensurePropertyName(propertyName) {
      if (properties[propertyName] == null) {
        properties[propertyName] = {
          astSpecificProperties: [],
        };
      }
    }
  });
}

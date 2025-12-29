(() => {
  var __defProp = Object.defineProperty;
  var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

  // scripts/nav-burger.js
  function addNavBurgerEvent() {
    const navBurger = document.getElementById("navbarBurger");
    navBurger.addEventListener("click", () => {
      navBurger.classList.toggle("is-active");
      document.getElementById(navBurger.dataset.target).classList.toggle("is-active");
    });
  }
  __name(addNavBurgerEvent, "addNavBurgerEvent");

  // scripts/plugin-config-table-replacer.js
  function replaceConfigTable() {
    const items = getPluginConfigTableItems();
    if (items.length > 0) {
      items.forEach(function(item) {
        getDprintPluginConfig(item.url).then((properties) => {
          const isOfficial = new URL(item.url).pathname.startsWith("/dprint/");
          const element = item.element;
          element.innerHTML = '<p>This information was auto generated from <a href="' + item.url + '">' + item.url + "</a>.</p>";
          properties.forEach(function(property) {
            const propertyContainer = document.createElement("div");
            element.appendChild(propertyContainer);
            try {
              const propertyTitle = document.createElement("h2");
              if (isOfficial && property.name === "preferSingleLine") {
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
          __name(addDescription, "addDescription");
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
              const typeContainer = document.createElement("li");
              infoContainer.appendChild(typeContainer);
              const typePrefix = document.createElement("strong");
              typePrefix.textContent = "Type: ";
              typeContainer.appendChild(typePrefix);
              typeContainer.append(property.type);
              const defaultContainer = document.createElement("li");
              infoContainer.appendChild(defaultContainer);
              const defaultPrefix = document.createElement("strong");
              defaultPrefix.textContent = "Default: ";
              defaultContainer.appendChild(defaultPrefix);
              defaultContainer.append(valueToText(property.default));
            }
          }
          __name(addInfoContainer, "addInfoContainer");
          function valueToText(value) {
            if (typeof value === "string") {
              return '"' + value + '"';
            }
            if (value == null) {
              return "<not specified>";
            }
            return value.toString();
          }
          __name(valueToText, "valueToText");
        });
      });
    }
  }
  __name(replaceConfigTable, "replaceConfigTable");
  function getPluginConfigTableItems() {
    const result = [];
    const elements = document.getElementsByClassName("plugin-config-table");
    for (let i = 0; i < elements.length; i++) {
      const element = elements.item(i);
      result.push({
        element,
        url: element.dataset.url
      });
    }
    return result;
  }
  __name(getPluginConfigTableItems, "getPluginConfigTableItems");
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
              definition: isSameDefinition ? null : definition
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
      __name(setDefinitionForPropertyName, "setDefinitionForPropertyName");
      function ensurePropertyName(propertyName) {
        if (properties[propertyName] == null) {
          properties[propertyName] = {
            astSpecificProperties: []
          };
        }
      }
      __name(ensurePropertyName, "ensurePropertyName");
    });
  }
  __name(getDprintPluginConfig, "getDprintPluginConfig");

  // scripts/plugin-url-replacer.js
  var pluginInfoUrl = "https://plugins.dprint.dev/info.json";
  var schemaVersion = 4;
  var pluginPlaceholders = /* @__PURE__ */ new Map([
    ['"https://plugins.dprint.dev/typescript-x.x.x.wasm"', "dprint-plugin-typescript"],
    ['"https://plugins.dprint.dev/json-x.x.x.wasm"', "dprint-plugin-json"],
    ['"https://plugins.dprint.dev/markdown-x.x.x.wasm"', "dprint-plugin-markdown"],
    ['"https://plugins.dprint.dev/toml-x.x.x.wasm"', "dprint-plugin-toml"],
    ['"https://plugins.dprint.dev/dockerfile-x.x.x.wasm"', "dprint-plugin-dockerfile"],
    ['"https://plugins.dprint.dev/biome-x.x.x.wasm"', "dprint-plugin-biome"],
    ['"https://plugins.dprint.dev/oxc-x.x.x.wasm"', "dprint-plugin-oxc"],
    ['"https://plugins.dprint.dev/ruff-x.x.x.wasm"', "dprint-plugin-ruff"],
    ['"https://plugins.dprint.dev/jupyter-x.x.x.wasm"', "dprint-plugin-jupyter"],
    ['"https://plugins.dprint.dev/g-plane/malva-vx.x.x.wasm"', "g-plane/malva"],
    ['"https://plugins.dprint.dev/g-plane/markup_fmt-vx.x.x.wasm"', "g-plane/markup_fmt"],
    ['"https://plugins.dprint.dev/g-plane/pretty_yaml-vx.x.x.wasm"', "g-plane/pretty_yaml"],
    ['"https://plugins.dprint.dev/g-plane/pretty_graphql-vx.x.x.wasm"', "g-plane/pretty_graphql"]
  ]);
  function replacePluginUrls() {
    const elements = getPluginUrlElements();
    if (elements.length > 0) {
      getPluginInfo().then((pluginUrls) => {
        for (const element of getPluginUrlElements()) {
          const pluginName = pluginPlaceholders.get(element.textContent);
          const url = pluginUrls.get(pluginName);
          if (url != null) {
            element.textContent = '"' + url + '"';
          }
        }
      });
    }
  }
  __name(replacePluginUrls, "replacePluginUrls");
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
  __name(getPluginUrlElements, "getPluginUrlElements");
  function getPluginInfo() {
    return fetch(pluginInfoUrl).then((response) => response.json()).then((data) => {
      if (data.schemaVersion !== schemaVersion) {
        throw new Error("Expected schema version " + schemaVersion + ", but found " + data.schemaVersion);
      }
      const result = /* @__PURE__ */ new Map();
      for (const pluginInfo of data.latest) {
        result.set(pluginInfo.name, pluginInfo.url);
      }
      return result;
    });
  }
  __name(getPluginInfo, "getPluginInfo");

  // scripts.js
  if (document.readyState === "complete" || document.readyState === "interactive") {
    setTimeout(onLoad, 0);
  } else {
    document.addEventListener("DOMContentLoaded", onLoad);
  }
  function onLoad() {
    replacePluginUrls();
    replaceConfigTable();
    addNavBurgerEvent();
  }
  __name(onLoad, "onLoad");
})();

(() => {
  var __defProp = Object.defineProperty;
  var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

  // deno:file:///home/runner/work/dprint/dprint/website/src/scripts/nav-burger.js
  function addNavBurgerEvent() {
    const navBurger = document.getElementById("navbarBurger");
    navBurger.addEventListener("click", () => {
      navBurger.classList.toggle("is-active");
      document.getElementById(navBurger.dataset.target).classList.toggle("is-active");
    });
  }
  __name(addNavBurgerEvent, "addNavBurgerEvent");

  // deno:file:///home/runner/work/dprint/dprint/website/src/scripts/plugin-config-table-replacer.js
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

  // deno:file:///home/runner/work/dprint/dprint/website/src/scripts/plugin-url-replacer.js
  var typescriptUrl = "https://plugins.dprint.dev/typescript-x.x.x.wasm";
  var jsonUrl = "https://plugins.dprint.dev/json-x.x.x.wasm";
  var markdownUrl = "https://plugins.dprint.dev/markdown-x.x.x.wasm";
  var tomlUrl = "https://plugins.dprint.dev/toml-x.x.x.wasm";
  var dockerfileUrl = "https://plugins.dprint.dev/dockerfile-x.x.x.wasm";
  var biomeUrl = "https://plugins.dprint.dev/biome-x.x.x.wasm";
  var ruffUrl = "https://plugins.dprint.dev/ruff-x.x.x.wasm";
  var jupyterUrl = "https://plugins.dprint.dev/jupyter-x.x.x.wasm";
  var malvaUrl = "https://plugins.dprint.dev/g-plane/malva-vx.x.x.wasm";
  var markupFmtUrl = "https://plugins.dprint.dev/g-plane/markup_fmt-vx.x.x.wasm";
  var yamlUrl = "https://plugins.dprint.dev/g-plane/pretty_yaml-vx.x.x.wasm";
  var graphqlUrl = "https://plugins.dprint.dev/g-plane/pretty_graphql-vx.x.x.wasm";
  var pluginInfoUrl = "https://plugins.dprint.dev/info.json";
  var schemaVersion = 4;
  function replacePluginUrls() {
    const elements = getPluginUrlElements();
    if (elements.length > 0) {
      getPluginInfo().then((urls) => {
        for (let i = 0; i < elements.length; i++) {
          const element = elements[i];
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
            case getWithQuotes(tomlUrl):
              element.textContent = getWithQuotes(urls["toml"]);
              break;
            case getWithQuotes(dockerfileUrl):
              element.textContent = getWithQuotes(urls["dockerfile"]);
              break;
            case getWithQuotes(biomeUrl):
              element.textContent = getWithQuotes(urls["biome"]);
              break;
            case getWithQuotes(ruffUrl):
              element.textContent = getWithQuotes(urls["ruff"]);
              break;
            case getWithQuotes(jupyterUrl):
              element.textContent = getWithQuotes(urls["jupyter"]);
              break;
            case getWithQuotes(malvaUrl):
              element.textContent = getWithQuotes(urls["malva"]);
              break;
            case getWithQuotes(markupFmtUrl):
              element.textContent = getWithQuotes(urls["markup_fmt"]);
              break;
            case getWithQuotes(yamlUrl):
              element.textContent = getWithQuotes(urls["yaml"]);
              break;
            case getWithQuotes(graphqlUrl):
              element.textContent = getWithQuotes(urls["graphql"]);
              break;
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
      switch (stringElement.textContent) {
        case getWithQuotes(typescriptUrl):
        case getWithQuotes(jsonUrl):
        case getWithQuotes(markdownUrl):
        case getWithQuotes(tomlUrl):
        case getWithQuotes(dockerfileUrl):
        case getWithQuotes(biomeUrl):
        case getWithQuotes(ruffUrl):
        case getWithQuotes(jupyterUrl):
        case getWithQuotes(malvaUrl):
        case getWithQuotes(markupFmtUrl):
        case getWithQuotes(yamlUrl):
        case getWithQuotes(graphqlUrl):
          result.push(stringElement);
          break;
      }
    }
    return result;
  }
  __name(getPluginUrlElements, "getPluginUrlElements");
  function getWithQuotes(text) {
    return '"' + text + '"';
  }
  __name(getWithQuotes, "getWithQuotes");
  function getPluginInfo() {
    return fetch(pluginInfoUrl).then((response) => {
      return response.json();
    }).then((data) => {
      if (data.schemaVersion !== schemaVersion) {
        throw new Error("Expected schema version " + schemaVersion + ", but found " + data.schemaVersion);
      }
      return {
        typescript: getUrlForPlugin(data, "dprint-plugin-typescript"),
        json: getUrlForPlugin(data, "dprint-plugin-json"),
        markdown: getUrlForPlugin(data, "dprint-plugin-markdown"),
        toml: getUrlForPlugin(data, "dprint-plugin-toml"),
        dockerfile: getUrlForPlugin(data, "dprint-plugin-dockerfile"),
        biome: getUrlForPlugin(data, "dprint-plugin-biome"),
        ruff: getUrlForPlugin(data, "dprint-plugin-ruff"),
        jupyter: getUrlForPlugin(data, "dprint-plugin-jupyter"),
        malva: getUrlForPlugin(data, "g-plane/malva"),
        markup_fmt: getUrlForPlugin(data, "g-plane/markup_fmt"),
        yaml: getUrlForPlugin(data, "g-plane/pretty_yaml"),
        graphql: getUrlForPlugin(data, "g-plane/pretty_graphql")
      };
    });
    function getUrlForPlugin(data, pluginName) {
      const pluginInfo = data.latest.find((pluginInfo2) => {
        return pluginInfo2.name === pluginName;
      });
      if (pluginInfo == null) {
        throw new Error("Could not find plugin with name " + pluginName);
      }
      return pluginInfo.url;
    }
    __name(getUrlForPlugin, "getUrlForPlugin");
  }
  __name(getPluginInfo, "getPluginInfo");

  // deno:file:///home/runner/work/dprint/dprint/website/src/scripts.js
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

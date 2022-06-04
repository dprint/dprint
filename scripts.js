(() => {
  var __defProp = Object.defineProperty;
  var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

  // src/scripts/nav-burger.js
  function addNavBurgerEvent() {
    const navBurger = document.getElementById("navbarBurger");
    navBurger.addEventListener("click", () => {
      navBurger.classList.toggle("is-active");
      document.getElementById(navBurger.dataset.target).classList.toggle("is-active");
    });
  }
  __name(addNavBurgerEvent, "addNavBurgerEvent");

  // src/scripts/plugin-config-table-replacer.js
  function replaceConfigTable() {
    const items = getPluginConfigTableItems();
    if (items.length > 0) {
      items.forEach(function(item) {
        getDprintPluginConfig(item.url).then((properties) => {
          const element = item.element;
          element.innerHTML = '<p>This information was auto generated from <a href="' + item.url + '">' + item.url + "</a>.</p>";
          properties.forEach(function(property) {
            const propertyContainer = document.createElement("div");
            element.appendChild(propertyContainer);
            try {
              const propertyTitle = document.createElement("h2");
              if (property.name === "preferSingleLine") {
                property.name += " (Very Experimental)";
              }
              propertyTitle.textContent = property.name;
              propertyContainer.appendChild(propertyTitle);
              const propertyDesc = document.createElement("p");
              propertyDesc.textContent = property.description;
              propertyContainer.appendChild(propertyDesc);
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
              if (property.astSpecificProperties != null && property.astSpecificProperties.length > 0) {
                const astSpecificPropertiesPrefix = document.createElement("p");
                astSpecificPropertiesPrefix.textContent = "AST node specific configuration property names:";
                propertyContainer.appendChild(astSpecificPropertiesPrefix);
                const astSpecificPropertyNamesContainer = document.createElement("ul");
                propertyContainer.appendChild(astSpecificPropertyNamesContainer);
                property.astSpecificProperties.forEach(function(propName) {
                  const propertyNameLi = document.createElement("li");
                  propertyNameLi.textContent = valueToText(propName);
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
          function valueToText(value) {
            if (typeof value === "string") {
              return '"' + value + '"';
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
          const definition = json.definitions[propertyName];
          if (definition != null) {
            setDefinitionForPropertyName(propertyName, definition);
          } else {
            const derivedPropName = property["$ref"].replace("#/definitions/", "");
            if (json.properties[derivedPropName] == null) {
              setDefinitionForPropertyName(propertyName, json.definitions[derivedPropName]);
            } else {
              ensurePropertyName(derivedPropName);
              properties[derivedPropName].astSpecificProperties.push(propertyName);
            }
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

  // src/scripts/plugin-url-replacer.js
  var typescriptUrl = "https://plugins.dprint.dev/typescript-x.x.x.wasm";
  var jsonUrl = "https://plugins.dprint.dev/json-x.x.x.wasm";
  var markdownUrl = "https://plugins.dprint.dev/markdown-x.x.x.wasm";
  var tomlUrl = "https://plugins.dprint.dev/toml-x.x.x.wasm";
  var dockerfileUrl = "https://plugins.dprint.dev/dockerfile-x.x.x.wasm";
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
        dockerfile: getUrlForPlugin(data, "dprint-plugin-dockerfile")
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

  // src/scripts.js
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

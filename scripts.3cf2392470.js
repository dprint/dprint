(() => {
  var __defProp = Object.defineProperty;
  var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

  // scripts/anchor-scroll.js
  function restoreAnchorScroll() {
    const target = getHashTarget();
    if (target == null || document.fonts == null) {
      return;
    }
    const scrollYBefore = window.scrollY;
    document.fonts.ready.then(() => {
      if (window.scrollY === scrollYBefore && getHashTarget() === target) {
        target.scrollIntoView();
      }
    });
  }
  __name(restoreAnchorScroll, "restoreAnchorScroll");
  function getHashTarget() {
    if (location.hash.length <= 1) {
      return null;
    }
    return document.getElementById(decodeURIComponent(location.hash.slice(1)));
  }
  __name(getHashTarget, "getHashTarget");

  // scripts/doc-menu-toggle.js
  function setupDocMenu() {
    const details = document.querySelector("details.doc-nav");
    if (details == null) {
      return;
    }
    if (window.matchMedia("(max-width: 860px)").matches) {
      details.open = false;
    }
  }
  __name(setupDocMenu, "setupDocMenu");

  // scripts/install-tabs.js
  var commands = {
    shell: "curl -fsSL https://dprint.dev/install.sh | sh",
    pwsh: "irm https://dprint.dev/install.ps1 | iex",
    npm: "npm install -g dprint",
    brew: "brew install dprint",
    cargo: "cargo install --locked dprint"
  };
  function addInstallTabsEvent() {
    const tabs = document.querySelectorAll(".os-tab");
    const cmdText = document.getElementById("cmd-text");
    const copyBtn = document.getElementById("copy-btn");
    if (tabs.length === 0 || cmdText == null) {
      return;
    }
    tabs.forEach(function(tab) {
      tab.addEventListener("click", function() {
        tabs.forEach(function(t) {
          t.classList.remove("active");
        });
        tab.classList.add("active");
        const os = tab.getAttribute("data-os");
        if (commands[os] != null) {
          cmdText.textContent = commands[os];
        }
        if (copyBtn != null) {
          copyBtn.textContent = "copy";
        }
      });
    });
    if (copyBtn != null) {
      let copyTimeout;
      copyBtn.addEventListener("click", function() {
        if (navigator.clipboard != null) {
          navigator.clipboard.writeText(cmdText.textContent).catch(function() {
          });
        }
        copyBtn.textContent = "copied \u2713";
        clearTimeout(copyTimeout);
        copyTimeout = setTimeout(function() {
          copyBtn.textContent = "copy";
        }, 1600);
      });
    }
  }
  __name(addInstallTabsEvent, "addInstallTabsEvent");

  // scripts/nav-height.js
  function setupNavHeight() {
    const nav = document.querySelector(".site-nav");
    if (nav == null) {
      return;
    }
    update();
    if (typeof ResizeObserver !== "undefined") {
      new ResizeObserver(update).observe(nav);
    } else {
      window.addEventListener("resize", update);
    }
    function update() {
      const height = Math.round(nav.getBoundingClientRect().height);
      document.documentElement.style.setProperty("--nav-h", `${height}px`);
    }
    __name(update, "update");
  }
  __name(setupNavHeight, "setupNavHeight");

  // scripts/plugin-config-table-replacer.js
  function replaceConfigTable() {
    const items = getPluginConfigTableItems();
    if (items.length > 0) {
      items.forEach(function(item) {
        getDprintPluginConfig(item.url, item.configKey).then((properties) => {
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
            if (property.description == null) {
              return;
            }
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
            } else if (property.enum) {
              property.enum.forEach(function(value) {
                const enumContainer = document.createElement("li");
                infoContainer.appendChild(enumContainer);
                const prefix = document.createElement("strong");
                prefix.textContent = valueToText(value);
                enumContainer.appendChild(prefix);
                if (value === property.default) {
                  enumContainer.append(" (Default)");
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
        url: element.dataset.url,
        // set when the schema nests the plugin config under its config key
        configKey: element.dataset.configKey
      });
    }
    return result;
  }
  __name(getPluginConfigTableItems, "getPluginConfigTableItems");
  function getDprintPluginConfig(configSchemaUrl, configKey) {
    return fetch(configSchemaUrl).then((response) => {
      return response.json();
    }).then((rootJson) => {
      const json = configKey == null ? rootJson : rootJson.properties[configKey];
      const definitions = rootJson.definitions || rootJson["$defs"] || {};
      const properties = {};
      let order = 0;
      for (const propertyName of Object.keys(json.properties)) {
        if (propertyName === "$schema" || propertyName === "deno" || propertyName === "locked") {
          continue;
        }
        const property = json.properties[propertyName];
        if (property["$ref"]) {
          const derivedPropName = property["$ref"].replace(/^#\/(definitions|\$defs)\//, "");
          const lastSegment = propertyName.split(".").pop();
          let parentProperty;
          if (derivedPropName !== propertyName && derivedPropName in json.properties) {
            parentProperty = derivedPropName;
          } else if (lastSegment !== propertyName && lastSegment in json.properties) {
            parentProperty = lastSegment;
          }
          const definition = definitions[derivedPropName];
          if (parentProperty) {
            ensurePropertyName(parentProperty);
            const isSameDefinition = property["$ref"] === json.properties[parentProperty]["$ref"];
            properties[parentProperty].astSpecificProperties.push({
              propertyName,
              definition: isSameDefinition ? null : definition
            });
          } else {
            setDefinitionForPropertyName(propertyName, Object.assign({}, definition, property));
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
  var npmPlaceholderRe = /^"npm:(.+)@x\.x\.x"$/;
  var pluginPlaceholders = /* @__PURE__ */ new Map([
    ['"https://plugins.dprint.dev/jolars/panache-x.x.x.wasm"', "jolars/panache"],
    ['"https://plugins.dprint.dev/jolars/badness-vx.x.x.wasm"', "jolars/badness"],
    ['"https://plugins.dprint.dev/jolars/arity-vx.x.x.wasm"', "jolars/arity"],
    ['"https://plugins.dprint.dev/jolars/fatou-vx.x.x.wasm"', "jolars/fatou"],
    ['"https://plugins.dprint.dev/apcamargo/typstyle-x.x.x.wasm"', "apcamargo/typstyle"],
    ['"https://plugins.dprint.dev/apcamargo/bibtex-tidy-x.x.x.wasm"', "apcamargo/bibtex-tidy"],
    ['"https://plugins.dprint.dev/sargunv/dprint-clang-format-x.x.x.wasm"', "sargunv/dprint-clang-format"],
    ['"https://plugins.dprint.dev/sargunv/dprint-cmakefmt-x.x.x.wasm"', "sargunv/dprint-cmakefmt"]
  ]);
  function replacePluginUrls() {
    const elements = getPluginUrlElements();
    if (elements.length > 0) {
      getPluginInfo().then(({ pluginUrls, npmVersions }) => {
        var _a;
        for (const element of elements) {
          const npmName = (_a = npmPlaceholderRe.exec(element.textContent)) == null ? void 0 : _a[1];
          if (npmName != null) {
            const version = npmVersions.get(npmName);
            if (version != null) {
              element.textContent = '"npm:' + npmName + "@" + version + '"';
            }
            continue;
          }
          const pluginName = pluginPlaceholders.get(element.textContent);
          const url = pluginUrls.get(pluginName);
          if (url != null) {
            element.textContent = '"' + url + '"';
          } else if (pluginName != null) {
            getLatestPluginUrl(pluginName).then((fallbackUrl) => {
              if (fallbackUrl != null) {
                element.textContent = '"' + fallbackUrl + '"';
              }
            });
          }
        }
      });
    }
  }
  __name(replacePluginUrls, "replacePluginUrls");
  function getLatestPluginUrl(pluginName) {
    return fetch("https://plugins.dprint.dev/" + pluginName + "/latest.json").then((response) => response.ok ? response.json() : null).then((data) => data == null ? null : data.url).catch(() => null);
  }
  __name(getLatestPluginUrl, "getLatestPluginUrl");
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
  __name(getPluginUrlElements, "getPluginUrlElements");
  function getPluginInfo() {
    return fetch(pluginInfoUrl).then((response) => response.json()).then((data) => {
      if (data.schemaVersion !== schemaVersion) {
        throw new Error("Expected schema version " + schemaVersion + ", but found " + data.schemaVersion);
      }
      const pluginUrls = /* @__PURE__ */ new Map();
      const npmVersions = /* @__PURE__ */ new Map();
      for (const pluginInfo of data.latest) {
        pluginUrls.set(pluginInfo.name, pluginInfo.url);
        if (pluginInfo.npm != null) {
          npmVersions.set(pluginInfo.npm.name, pluginInfo.npm.version);
        }
      }
      return { pluginUrls, npmVersions };
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
    setupNavHeight();
    restoreAnchorScroll();
    replacePluginUrls();
    replaceConfigTable();
    addInstallTabsEvent();
    setupDocMenu();
  }
  __name(onLoad, "onLoad");
})();

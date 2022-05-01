import { addNavBurgerEvent } from "./scripts/nav-burger.js";
import { replaceConfigTable } from "./scripts/plugin-config-table-replacer.js";
import { replacePluginUrls } from "./scripts/plugin-url-replacer.js";

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

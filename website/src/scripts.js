import { restoreAnchorScroll } from "./scripts/anchor-scroll.js";
import { setupDocMenu } from "./scripts/doc-menu-toggle.js";
import { addInstallTabsEvent } from "./scripts/install-tabs.js";
import { setupNavHeight } from "./scripts/nav-height.js";
import { replaceConfigTable } from "./scripts/plugin-config-table-replacer.js";
import { replacePluginUrls } from "./scripts/plugin-url-replacer.js";

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

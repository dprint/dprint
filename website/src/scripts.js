import { restoreAnchorScroll } from "./scripts/anchor-scroll.js";
import { setupCopyButtons } from "./scripts/copy-buttons.js";
import { setupDocMenu } from "./scripts/doc-menu-toggle.js";
import { addInstallTabsEvent } from "./scripts/install-tabs.js";
import { setupNavHeight } from "./scripts/nav-height.js";
import { openHashDetails } from "./scripts/open-hash-details.js";

if (document.readyState === "complete" || document.readyState === "interactive") {
  setTimeout(onLoad, 0);
} else {
  document.addEventListener("DOMContentLoaded", onLoad);
}

function onLoad() {
  setupNavHeight();
  openHashDetails();
  restoreAnchorScroll();
  addInstallTabsEvent();
  setupDocMenu();
  setupCopyButtons();
}

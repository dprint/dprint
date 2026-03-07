// @ts-check
"use strict";

const api = require("./install_api");
const exePath = api.runInstall();
try {
  api.replaceBinEntry(exePath);
} catch (_err) {
  // ignore - falls back to bin.js
}

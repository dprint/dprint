#!/usr/bin/env node

// @ts-check
const path = require("path");
const child_process = require("child_process");
const os = require("os");
const fs = require("fs");

const exePath = path.join(__dirname, os.platform() === "win32" ? "dprint.exe" : "dprint");

if (!fs.existsSync(exePath)) {
  try {
    const resolvedExePath = require("./install_api").runInstall();
    runDprintExe(resolvedExePath);
  } catch (err) {
    if (err !== undefined && typeof err.message === "string") {
      console.error(err.message);
    } else {
      console.error(err);
    }
    process.exit(1);
  }
} else {
  runDprintExe(exePath);
}

/** @param exePath {string} */
function runDprintExe(exePath) {
  const result = child_process.spawnSync(
    exePath,
    process.argv.slice(2),
    { stdio: "inherit" },
  );
  if (result.error) {
    throw result.error;
  }

  throwIfNoExePath();

  process.exitCode = result.status;

  function throwIfNoExePath() {
    if (!fs.existsSync(exePath)) {
      throw new Error("Could not find exe at path '" + exePath + "'. Maybe try running dprint again.");
    }
  }
}

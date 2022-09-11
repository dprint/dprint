#!/usr/bin/env node

// @ts-check
const path = require("path");
const child_process = require("child_process");
const os = require("os");
const fs = require("fs");

const exePath = path.join(__dirname, os.platform() === "win32" ? "dprint.exe" : "dprint");

if (!fs.existsSync(exePath)) {
  require("./install_api").runInstall().then(() => {
    runDprintExe();
  }).catch(err => {
    console.error(err);
    process.exit(child_process.exitCode || 1);
  });
} else {
  runDprintExe();
}

function runDprintExe() {
  try {
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
  } catch (err) {
    throw err;
  }
}

function throwIfNoExePath() {
  if (!fs.existsSync(exePath)) {
    throw new Error("Could not find exe at path '" + exePath + "'. Maybe try running dprint again.");
  }
}

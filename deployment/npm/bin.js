#!/usr/bin/env node

// @ts-check
const path = require("path");
const child_process = require("child_process");
const os = require("os");
const fs = require("fs");

const exePath = path.join(__dirname, os.platform() === "win32" ? "dprint.exe" : "dprint");

try {
    const result = child_process.spawnSync(
        exePath,
        process.argv.slice(2),
        { stdio: "inherit" },
    );

    if (result.status !== 0) {
        throwIfNoExePath();
    }

    process.exitCode = result.status;
} catch (err) {
    throwIfNoExePath();
    throw err;
}

function throwIfNoExePath() {
    if (!fs.existsSync(exePath)) {
        throw new Error("Could not find exe at path '" + exePath + "'. Please ensure the dprint 'postinstall' script runs on install.");
    }
}

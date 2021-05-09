// @ts-check
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const child_process = require("child_process");

const version = "0.13.1";

if (os.platform() === "win32") {
    if (!fs.existsSync("dprint.exe")) {
        const result = child_process.spawnSync("powershell.exe", [
            "-noprofile",
            "-file",
            path.join(__dirname, "install.ps1"),
            version,
        ], {
            stdio: "inherit",
        });
        process.exitCode = result.status;
    }
} else {
    if (!fs.existsSync("dprint")) {
        const installScriptPath = path.join(__dirname, "install.sh");
        fs.chmodSync(installScriptPath, "755");
        child_process.execSync(`${installScriptPath} ${version}`, { stdio: "inherit" });
    }
}

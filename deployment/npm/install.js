// @ts-check
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const child_process = require("child_process");

const info = JSON.parse(fs.readFileSync(path.join(__dirname, "info.json"), "utf8"));
const platform = os.platform();

if (os.arch() !== "x64") {
    throw new Error("Unsupported architecture " + os.arch() + ". Only x64 binaries are available.");
}

if (platform === "win32") {
    if (!fs.existsSync("dprint.exe")) {
        const result = child_process.spawnSync("powershell.exe", [
            "-noprofile",
            "-file",
            path.join(__dirname, "install.ps1"),
            info.version,
            getZipChecksum(),
        ], {
            stdio: "inherit",
            cwd: __dirname,
        });
        process.exitCode = result.status;
    }
} else {
    if (!fs.existsSync("dprint")) {
        const installScriptPath = path.join(__dirname, "install.sh");
        fs.chmodSync(installScriptPath, "755");
        child_process.execSync(`${installScriptPath} ${info.version} ${getZipChecksum()}`, {
            stdio: "inherit",
            cwd: __dirname,
        });
    }
}

function getZipChecksum() {
    switch (platform) {
        case "win32":
            return info.checksums.windows;
        case "darwin":
            return info.checksums.mac;
        default:
            return info.checksums.linux;
    }
}

// @ts-check
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const child_process = require("child_process");

const version = "0.13.1";

if (os.platform() === "win32") {
    if (!fs.existsSync("dprint.exe")) {
        const child = child_process.spawn("powershell.exe", [
            "-noprofile",
            "-file",
            path.join(__dirname, "install.ps1"),
            version,
        ]);
        child.stdout.on("data", data => {
            console.log(data.toString());
        });
        child.stderr.on("data", data => {
            console.error(data.toString());
        });
        child.stdin.end();
    }
} else {
    if (!fs.existsSync("dprint")) {
        child_process.exec(`./install.sh ${version}`);
    }
}

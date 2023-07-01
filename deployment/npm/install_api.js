// @ts-check
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
/** @type {string | undefined} */
let cachedIsMusl = undefined;

module.exports = {
  runInstall() {
    const dprintFileName = os.platform() === "win32" ? "dprint.exe" : "dprint";
    const executableFilePath = path.join(
      __dirname,
      dprintFileName,
    );

    if (fs.existsSync(executableFilePath)) {
      return Promise.resolve();
    }

    const executablePath = path.join("../", "@dprint", getTarget(), dprintFileName);
    if (!fs.existsSync(executablePath)) {
    }
    fs.copyFileSync(executableFilePath, executablePath);

    function getTarget() {
      const platform = os.platform();
      if (platform === "linux") {
        return `${platform}-${getArch()}-${getLinuxFamily()}`;
      } else {
        return `${platform}-${getArch()}`;
      }
    }

    function getArch() {
      const arch = os.arch();
      if (arch !== "arm64" && arch !== "x64") {
        throw new Error("Unsupported architecture " + os.arch() + ". Only x64 and aarch64 binaries are available.");
      }
      return arch;
    }

    function getLinuxFamily() {
      return getIsMusl() ? "musl" : "glibc";

      function getIsMusl() {
        // code adapted from https://github.com/lovell/detect-libc
        // Copyright Apache 2.0 license, the detect-libc maintainers
        if (cachedIsMusl == null) {
          cachedIsMusl = innerGet();
        }
        return cachedIsMusl;

        function innerGet() {
          try {
            if (os.platform() !== "linux") {
              return false;
            }
            return isProcessReportMusl() || isConfMusl();
          } catch (err) {
            // just in case
            console.warn("Error checking if musl.", err);
            return false;
          }
        }

        function isProcessReportMusl() {
          if (!process.report) {
            return false;
          }
          const rawReport = process.report.getReport();
          const report = typeof rawReport === "string" ? JSON.parse(rawReport) : rawReport;
          if (!report || !(report.sharedObjects instanceof Array)) {
            return false;
          }
          return report.sharedObjects.some(o => o.includes("libc.musl-") || o.includes("ld-musl-"));
        }

        function isConfMusl() {
          const output = getCommandOutput();
          const [_, ldd1] = output.split(/[\r\n]+/);
          return ldd1 && ldd1.includes("musl");
        }

        function getCommandOutput() {
          try {
            const command = "getconf GNU_LIBC_VERSION 2>&1 || true; ldd --version 2>&1 || true";
            return require("child_process").execSync(command, { encoding: "utf8" });
          } catch (_err) {
            return "";
          }
        }
      }
    }
  },
};

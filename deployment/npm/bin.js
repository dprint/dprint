#!/usr/bin/env node
// @ts-check
"use strict";

const child_process = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const isDebug = process.env.DPRINT_DEBUG === "1";
/** @type {{ didChmod?: true; isMusl?: boolean; } | undefined} */
let cacheData = undefined;

runDprintExe(resolvePath());

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
      clearCacheData();
      // clear the cache
      throw new Error("Could not find exe at path '" + exePath + "'. Maybe try running dprint again.");
    }
  }
}

function resolvePath() {
  const dprintFileName = os.platform() === "win32" ? "dprint.exe" : "dprint";
  const target = getTarget();
  const sourcePackagePath = path.dirname(require.resolve("@dprint/" + target + "/package.json"));
  const sourceExecutablePath = path.join(sourcePackagePath, dprintFileName);

  if (os.platform() !== "win32") {
    const cacheData = getCacheData();
    if (!cacheData.didChmod) {
      chmodX(sourceExecutablePath);
      cacheData.didChmod = true;
      saveCacheData();
    }
  }

  return sourceExecutablePath;
}

/** @filePath {string} */
function chmodX(filePath) {
  const perms = fs.statSync(filePath).mode;
  fs.chmodSync(filePath, perms | 0o111);
}

function getTarget() {
  const platform = os.platform();
  if (platform === "linux") {
    return platform + "-" + getArch() + "-" + getLinuxFamily();
  } else {
    return platform + "-" + getArch();
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
    const cacheData = getCacheData();
    // code adapted from https://github.com/lovell/detect-libc
    // Copyright Apache 2.0 license, the detect-libc maintainers
    if (cacheData.isMusl == null) {
      cacheData.isMusl = innerGet();
      saveCacheData();
    }
    return cacheData.isMusl;

    /** @returns {boolean} */
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

/** @returns {NonNullable<typeof cacheData>} */
function getCacheData() {
  if (cacheData != null) {
    return cacheData;
  }
  try {
    return fs.readFileSync(getCacheDataFile(), "utf8");
  } catch (err) {
    if (isDebug) {
      console.warn("Error getting cache data.", err);
    }
    cacheData = {};
    return cacheData;
  }
}

function clearCacheData() {
  cacheData = {};
  saveCacheData();
}

function saveCacheData() {
  try {
    cacheData = cacheData ?? {};
    atomicWriteFileSync(getCacheDataFile(), JSON.stringify(cacheData ?? {}));
  } catch (err) {
    if (isDebug) {
      console.warn("Error saving cache data.", err);
    }
  }
}

/**
 * Writes to the file system at a temporary path, then does a rename to
 * allow multiple processes to write to the same file system without
 * corrupting the underlying data.
 * @param filePath {string}
 * @param text {string}
 */
function atomicWriteFileSync(filePath, text) {
  const crypto = require("crypto");
  const rand = crypto.randomBytes(4).toString("hex");
  const tempFilePath = filePath + "." + rand;
  fs.writeFileSync(tempFilePath, text, "utf8");
  fs.renameSync(tempFilePath, filePath);
}

function getCacheDataFile() {
  return path.join(__dirname, "cache.json");
}

// @ts-check
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
/** @type {boolean | undefined} */
let cachedIsMusl = undefined;

module.exports = {
  replaceBinEntry,
  runInstall() {
    const dprintFileName = os.platform() === "win32" ? "dprint.exe" : "dprint";
    const targetExecutablePath = path.join(
      __dirname,
      dprintFileName,
    );

    if (fs.existsSync(targetExecutablePath)) {
      return targetExecutablePath;
    }

    const target = getTarget();
    const sourceExecutablePath = resolveSourceExecutablePath(target, dprintFileName);

    if (sourceExecutablePath == null) {
      // the @dprint/<target> optional dependency isn't installed (for example
      // the user ran `npm install --omit=optional`), so download the binary
      // directly from the registry like esbuild does
      downloadExecutable(target, dprintFileName, targetExecutablePath);
      if (os.platform() !== "win32") {
        // chmod +x
        chmodX(targetExecutablePath);
      }
      return targetExecutablePath;
    }

    try {
      if (process.env.DPRINT_SIMULATED_READONLY_FILE_SYSTEM === "1") {
        console.warn("Simulating readonly file system for testing.");
        throw new Error("Throwing for testing purposes.");
      }

      // in order to make things faster the next time we run and to allow the
      // dprint vscode extension to easily pick this up, copy the executable
      // into the dprint package folder
      hardLinkOrCopy(sourceExecutablePath, targetExecutablePath);
      if (os.platform() !== "win32") {
        // chmod +x
        chmodX(targetExecutablePath);
      }
      return targetExecutablePath;
    } catch (err) {
      // this may fail on readonly file systems... in this case, fall
      // back to using the resolved package path
      if (process.env.DPRINT_DEBUG === "1") {
        console.warn(
          "Failed to copy executable from "
            + sourceExecutablePath + " to " + targetExecutablePath
            + ". Using resolved package path instead.",
          err,
        );
      }
      return sourceExecutablePath;
    }
  },
};

/**
 * Resolves the path to the executable provided by the @dprint/<target>
 * optional dependency, or undefined when that package isn't installed.
 * @param target {string}
 * @param dprintFileName {string}
 * @returns {string | undefined}
 */
function resolveSourceExecutablePath(target, dprintFileName) {
  let sourcePackagePath;
  try {
    sourcePackagePath = path.dirname(require.resolve("@dprint/" + target + "/package.json"));
  } catch {
    // the optional dependency wasn't installed
    return undefined;
  }
  const sourceExecutablePath = path.join(sourcePackagePath, dprintFileName);
  return fs.existsSync(sourceExecutablePath) ? sourceExecutablePath : undefined;
}

/**
 * Downloads the binary tarball for the target from the registry and extracts
 * the executable to the destination path.
 * @param target {string}
 * @param dprintFileName {string}
 * @param destinationPath {string}
 */
function downloadExecutable(target, dprintFileName, destinationPath) {
  const version = require("./package.json").version;
  const registry = (process.env.npm_config_registry || "https://registry.npmjs.org")
    .replace(/\/+$/, "");
  // npm tarball urls drop the scope from the file name (e.g.
  // https://registry.npmjs.org/@dprint/win32-x64/-/win32-x64-1.0.0.tgz)
  const tarballUrl = registry + "/@dprint/" + target + "/-/" + target + "-" + version + ".tgz";
  console.error("[dprint] Optional dependency @dprint/" + target + " was not installed. Downloading from " + tarballUrl);
  const tarballBuffer = downloadBufferSync(tarballUrl);
  // files inside an npm tarball live under the "package/" directory
  const executableBuffer = extractFileFromTarGzip(tarballBuffer, "package/" + dprintFileName);
  verifyExecutableHash(target, executableBuffer);
  atomicWriteFile(destinationPath, executableBuffer);
}

/**
 * Verifies the downloaded executable against the hash recorded at build time
 * so that we only ever run the exact binary that was published.
 * @param target {string}
 * @param buffer {Buffer}
 */
function verifyExecutableHash(target, buffer) {
  let hashes;
  try {
    hashes = require("./hashes.json");
  } catch (err) {
    throw new Error("Could not load hashes.json to verify the downloaded binary: " + (err && err.message || err));
  }
  const expected = hashes[target];
  if (typeof expected !== "string") {
    throw new Error("No known hash for @dprint/" + target + " to verify the download against.");
  }
  const actual = require("crypto").createHash("sha256").update(buffer).digest("hex");
  if (actual !== expected) {
    throw new Error(
      "Integrity check failed for the downloaded @dprint/" + target + " binary.\n"
        + "  Expected sha256: " + expected + "\n"
        + "  Actual sha256:   " + actual,
    );
  }
}

/**
 * Downloads a url to a buffer synchronously. Prefer curl since it transparently
 * supports proxies (http_proxy/https_proxy/no_proxy and npm's proxy config),
 * redirects and TLS, then fall back to node for environments without curl.
 * @param url {string}
 * @returns {Buffer}
 */
function downloadBufferSync(url) {
  const fromCurl = downloadBufferWithCurl(url);
  return fromCurl != null ? fromCurl : downloadBufferWithNode(url);
}

/**
 * @param url {string}
 * @returns {Buffer | undefined} undefined when curl isn't available
 */
function downloadBufferWithCurl(url) {
  // -f: fail on http errors, -s: silent, -S: still show errors, -L: follow redirects
  const args = ["-fsSL"];
  const proxy = process.env.npm_config_https_proxy
    || process.env.npm_config_proxy
    || process.env.HTTPS_PROXY
    || process.env.https_proxy;
  if (proxy) {
    args.push("--proxy", proxy);
  }
  const noProxy = process.env.npm_config_noproxy || process.env.NO_PROXY || process.env.no_proxy;
  if (noProxy) {
    args.push("--noproxy", noProxy);
  }
  args.push("-o", "-", url);
  try {
    return require("child_process").execFileSync("curl", args, { maxBuffer: 512 * 1024 * 1024 });
  } catch (err) {
    if (err && err.code === "ENOENT") {
      // curl isn't installed, fall back to node
      return undefined;
    }
    throw err;
  }
}

/**
 * Node has no synchronous https client, so spawn a child node process that
 * streams the response to stdout. Does not support proxies.
 * @param url {string}
 * @returns {Buffer}
 */
function downloadBufferWithNode(url) {
  const script = "const https=require('https');"
    + "function f(u){https.get(u,r=>{"
    + "const s=r.statusCode;"
    + "if((s===301||s===302||s===303||s===307||s===308)&&r.headers.location){f(new URL(r.headers.location,u).toString());return;}"
    + "if(s!==200){console.error('[dprint] Unexpected status code '+s+' downloading '+u);process.exit(1);}"
    + "r.pipe(process.stdout);"
    + "}).on('error',e=>{console.error(String(e&&e.message||e));process.exit(1);});}"
    + "f(process.argv[1]);";
  return require("child_process").execFileSync(
    process.execPath,
    ["-e", script, url],
    { maxBuffer: 512 * 1024 * 1024 },
  );
}

/**
 * Extracts a single file from a gzipped tarball buffer.
 * @param buffer {Buffer}
 * @param subpath {string}
 * @returns {Buffer}
 */
function extractFileFromTarGzip(buffer, subpath) {
  const zlib = require("zlib");
  let tar;
  try {
    tar = zlib.gunzipSync(buffer);
  } catch (err) {
    throw new Error("Invalid gzip data in downloaded tarball: " + (err && err.message || err));
  }
  let offset = 0;
  while (offset < tar.length) {
    const name = readTarString(tar, offset, 100);
    const size = parseInt(readTarString(tar, offset + 124, 12).trim(), 8);
    offset += 512;
    if (!isNaN(size)) {
      if (name === subpath) {
        return tar.subarray(offset, offset + size);
      }
      // entries are padded to 512 byte boundaries
      offset += (size + 511) & ~511;
    }
  }
  throw new Error("Could not find " + JSON.stringify(subpath) + " in downloaded tarball");
}

/**
 * @param buffer {Buffer}
 * @param offset {number}
 * @param length {number}
 */
function readTarString(buffer, offset, length) {
  return buffer.toString("utf8", offset, offset + length).replace(/\0.*$/, "");
}

/**
 * @param destinationPath {string}
 * @param buffer {Buffer}
 */
function atomicWriteFile(destinationPath, buffer) {
  const crypto = require("crypto");
  const rand = crypto.randomBytes(4).toString("hex");
  const tempFilePath = destinationPath + "." + rand;
  fs.writeFileSync(tempFilePath, buffer);
  try {
    fs.renameSync(tempFilePath, destinationPath);
  } catch (err) {
    // will maybe throw when another process had already done this
    // so just ignore and delete the created temporary file
    try {
      fs.unlinkSync(tempFilePath);
    } catch (_err2) {
      // ignore
    }
    throw err;
  }
}

/** @filePath {string} */
function chmodX(filePath) {
  const fd = fs.openSync(filePath, "r");
  try {
    const perms = fs.fstatSync(fd).mode;
    fs.fchmodSync(fd, perms | 0o111);
  } finally {
    fs.closeSync(fd);
  }
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
  if (arch !== "arm64" && arch !== "x64" && arch !== "riscv64" && arch !== "loong64") {
    throw new Error("Unsupported architecture " + os.arch() + ". Only x64, aarch64, riscv64 and loong64 binaries are available.");
  }
  return arch;
}

function getLinuxFamily() {
  return getIsMusl() ? "musl" : "glibc";

  function getIsMusl() {
    // code adapted from https://github.com/napi-rs/package-template/blob/main/index.js
    // which is in turn based on https://github.com/lovell/detect-libc (Apache 2.0 license)
    if (cachedIsMusl == null) {
      cachedIsMusl = innerGet();
    }
    return cachedIsMusl;

    function innerGet() {
      try {
        if (os.platform() !== "linux") {
          return false;
        }
        let musl = isMuslFromFilesystem();
        if (musl == null) {
          musl = isMuslFromReport();
        }
        if (musl == null) {
          musl = isMuslFromChildProcess();
        }
        return musl;
      } catch (err) {
        // just in case
        console.warn("Error checking if musl.", err);
        return false;
      }
    }

    function isMuslFromFilesystem() {
      try {
        return fs.readFileSync("/usr/bin/ldd", "utf-8").includes("musl");
      } catch {
        return null;
      }
    }

    function isMuslFromReport() {
      if (typeof process.report?.getReport !== "function") {
        return null;
      }
      // excludeNetwork avoids a slow reverse DNS lookup while generating the
      // report, but it's a global flag so restore it once we're done
      const originalExcludeNetwork = process.report.excludeNetwork;
      let rawReport;
      try {
        process.report.excludeNetwork = true;
        rawReport = process.report.getReport();
      } finally {
        process.report.excludeNetwork = originalExcludeNetwork;
      }
      const report = typeof rawReport === "string" ? JSON.parse(rawReport) : rawReport;
      if (!report) {
        return null;
      }
      if (report.header && report.header.glibcVersionRuntime) {
        return false;
      }
      if (Array.isArray(report.sharedObjects)) {
        return report.sharedObjects.some(isFileMusl);
      }
      return false;
    }

    function isMuslFromChildProcess() {
      try {
        return require("child_process").execSync("ldd --version", { encoding: "utf8" }).includes("musl");
      } catch {
        return false;
      }
    }

    function isFileMusl(f) {
      return f.includes("libc.musl-") || f.includes("ld-musl-");
    }
  }
}

/**
 * Replaces the bin entry in node_modules/.bin to point directly at the
 * native binary, avoiding Node.js startup overhead on each invocation.
 * @param exePath {string}
 */
function replaceBinEntry(exePath) {
  const binDir = findBinDir();
  if (binDir === undefined) return;

  const relative = path.relative(binDir, exePath);
  if (os.platform() === "win32") {
    // rewrite .cmd and .ps1 wrappers to invoke the native binary directly
    fs.writeFileSync(
      path.join(binDir, "dprint.cmd"),
      "@\"%~dp0" + relative + "\" %*\r\n",
    );
    fs.writeFileSync(
      path.join(binDir, "dprint.ps1"),
      "& \"$PSScriptRoot/" + relative.replace(/\\/g, "/")
        + "\" $args\r\nexit $LASTEXITCODE\r\n",
    );
  } else {
    // replace symlink to point directly at the native binary
    const binDprint = path.join(binDir, "dprint");
    fs.unlinkSync(binDprint);
    fs.symlinkSync(relative, binDprint);
  }
}

function findBinDir() {
  // For global installs, npm sets npm_config_global=true and npm_config_prefix
  // to the install prefix. The bin dir is {prefix}/bin on Linux/Mac or
  // {prefix} on Windows (e.g. %APPDATA%\npm).
  if (process.env.npm_config_global === "true") {
    const prefix = process.env.npm_config_prefix;
    if (prefix) {
      const binDir = os.platform() === "win32"
        ? prefix
        : path.join(prefix, "bin");
      if (isBinDirForThisPackage(binDir)) {
        return binDir;
      }
    }
  }

  // For local installs, walk up looking for node_modules/.bin
  let dir = __dirname;
  for (let i = 0; i < 64; i++) {
    const parent = path.dirname(dir);
    if (parent === dir) {
      break;
    }
    if (path.basename(parent) === "node_modules") {
      const binDir = path.join(parent, ".bin");
      if (isBinDirForThisPackage(binDir)) {
        return binDir;
      }
    }
    dir = parent;
  }
  return undefined;
}

function isBinDirForThisPackage(binDir) {
  try {
    if (os.platform() === "win32") {
      // verify the .cmd wrapper references our bin.cjs
      const content = fs.readFileSync(
        path.join(binDir, "dprint.cmd"),
        "utf8",
      );
      return content.includes("bin.cjs");
    } else {
      // verify the symlink points into our package directory
      const linkTarget = fs.readlinkSync(path.join(binDir, "dprint"));
      const resolved = path.resolve(binDir, linkTarget);
      return resolved.endsWith("bin.cjs");
    }
  } catch (_err) {
    return false;
  }
}

/**
 * @param sourcePath {string}
 * @param destinationPath {string}
 */
function hardLinkOrCopy(sourcePath, destinationPath) {
  try {
    fs.linkSync(sourcePath, destinationPath);
  } catch {
    atomicCopyFile(sourcePath, destinationPath);
  }
}

/**
 * @param sourcePath {string}
 * @param destinationPath {string}
 */
function atomicCopyFile(sourcePath, destinationPath) {
  const crypto = require("crypto");
  const rand = crypto.randomBytes(4).toString("hex");
  const tempFilePath = destinationPath + "." + rand;
  fs.copyFileSync(sourcePath, tempFilePath);
  try {
    fs.renameSync(tempFilePath, destinationPath);
  } catch (err) {
    // will maybe throw when another process had already done this
    // so just ignore and delete the created temporary file
    try {
      fs.unlinkSync(tempFilePath);
    } catch (_err2) {
      // ignore
    }
    throw err;
  }
}

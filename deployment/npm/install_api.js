// @ts-check
"use strict";

const crypto = require("crypto");
const fs = require("fs");
const https = require("https");
const os = require("os");
const path = require("path");
const url = require("url");
const HttpsProxyAgent = require("https-proxy-agent");
const yauzl = require("yauzl");
/** @type {string | undefined} */
let cachedIsMusl = undefined;

function install() {
  const executableFilePath = path.join(
    __dirname,
    os.platform() === "win32" ? "dprint.exe" : "dprint",
  );

  if (fs.existsSync(executableFilePath)) {
    return Promise.resolve();
  }

  const info = JSON.parse(fs.readFileSync(path.join(__dirname, "info.json"), "utf8"));
  const zipFilePath = path.join(__dirname, "dprint.zip");

  const target = getTarget();
  const downloadUrl = "https://github.com/dprint/dprint/releases/download/"
    + info.version
    + "/dprint-" + target + ".zip";

  // remove the old zip file if it exists
  try {
    fs.unlinkSync(zipFilePath);
  } catch (err) {
    // ignore
  }

  // now try to download it
  return downloadZipFileWithRetries(downloadUrl).then(() => {
    verifyZipChecksum();
    return extractZipFile().then(() => {
      // todo: how to just +x? does it matter?
      fs.chmodSync(executableFilePath, 0o755);

      // delete the zip file
      try {
        fs.unlinkSync(zipFilePath);
      } catch (err) {
        // ignore
      }
    }).catch(err => {
      throw new Error("Error extracting dprint zip file.\n\n" + err);
    });
  }).catch(err => {
    throw new Error("Error downloading dprint zip file.\n\n" + err);
  });

  function getTarget() {
    if (os.platform() === "win32") {
      return "x86_64-pc-windows-msvc";
    } else if (os.platform() === "darwin") {
      return `${getArch()}-apple-darwin`;
    } else {
      return `${getArch()}-unknown-linux-${getLinuxFamily()}`;
    }
  }

  function downloadZipFileWithRetries(url) {
    /** @param remaining {number} */
    function download(remaining) {
      return downloadZipFile(url)
        .catch(err => {
          if (remaining === 0) {
            return Promise.reject(err);
          } else {
            console.error("Error downloading dprint zip file.", err);
            console.error("Retrying download (remaining: " + remaining + ")");
            return download(remaining - 1);
          }
        });
    }

    return download(3);
  }

  function downloadZipFile(url) {
    return new Promise((resolve, reject) => {
      const options = {};
      const proxyUrl = getProxyUrl(url);
      if (proxyUrl != null) {
        options.agent = new HttpsProxyAgent(proxyUrl);
      } else {
        // Node 19+ defaults keepAlive to true for `https.get`, but contains a bug
        // that prevents the process from exiting. Work around this by explicitly
        // disabling keepAlive.
        //
        // See: https://github.com/nodejs/node/issues/47228
        options.agent = new https.Agent({ keepAlive: false });
      }

      https.get(url, options, function(response) {
        if (response.statusCode != null && response.statusCode >= 200 && response.statusCode <= 299) {
          downloadResponse(response).then(resolve).catch(reject);
        } else if (response.headers.location) {
          downloadZipFile(response.headers.location).then(resolve).catch(reject);
        } else {
          reject(new Error("Unknown status code " + response.statusCode + " : " + response.statusMessage));
        }
      }).on("error", function(err) {
        try {
          fs.unlinkSync(zipFilePath);
        } catch (err) {
          // ignore
        }
        reject(err);
      });
    });

    /** @param response {import("http").IncomingMessage} */
    function downloadResponse(response) {
      return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(zipFilePath);
        response.pipe(file);
        file.on("finish", function() {
          file.close((err) => {
            if (err) {
              reject(err);
            } else {
              resolve(undefined);
            }
          });
        });
      });
    }
  }

  function getProxyUrl(requestUrl) {
    try {
      const proxyUrl = process.env.HTTPS_PROXY || process.env.HTTP_PROXY;
      if (typeof proxyUrl !== "string" || proxyUrl.length === 0) {
        return undefined;
      }
      if (typeof process.env.NO_PROXY === "string") {
        const noProxyAddresses = process.env.NO_PROXY.split(",");
        const host = url.parse(requestUrl).host;
        if (host == null || noProxyAddresses.indexOf(host) >= 0) {
          return undefined;
        }
      }
      return proxyUrl;
    } catch (err) {
      console.error("[dprint]: Error getting proxy url.", err);
      return undefined;
    }
  }

  function verifyZipChecksum() {
    const fileData = fs.readFileSync(zipFilePath);
    const actualZipChecksum = crypto.createHash("sha256").update(fileData).digest("hex").toLowerCase();
    const expectedZipChecksum = getExpectedZipChecksum().toLowerCase();

    if (actualZipChecksum !== expectedZipChecksum) {
      throw new Error(
        "Downloaded dprint zip checksum did not match the expected checksum (Actual: "
          + actualZipChecksum
          + ", Expected: "
          + expectedZipChecksum
          + ").",
      );
    }

    function getExpectedZipChecksum() {
      const checksum = info.checksums[getTarget()];
      if (checksum == null) {
        throw new Error("Could not find checksum for target: " + checksum);
      }
      return checksum;
    }
  }

  function extractZipFile() {
    return new Promise((resolve, reject) => {
      // code adapted from: https://github.com/thejoshwolfe/yauzl#usage
      yauzl.open(zipFilePath, { autoClose: true }, (err, zipFile) => {
        if (err) {
          reject(err);
          return;
        }

        const pendingWrites = [];

        zipFile.on("entry", (entry) => {
          if (!/\/$/.test(entry.fileName)) {
            // file entry

            // note: reject at the top level, but resolve this promise when finished
            pendingWrites.push(
              new Promise((resolve) => {
                zipFile.openReadStream(entry, (err, readStream) => {
                  if (err) {
                    reject(err);
                    return;
                  }
                  const destination = path.join(__dirname, entry.fileName);
                  const writeStream = fs.createWriteStream(destination);
                  readStream.pipe(writeStream);

                  writeStream.on("error", (err) => {
                    reject(err);
                  });
                  writeStream.on("finish", () => {
                    resolve(undefined);
                  });
                });
              }),
            );
          }
        });

        zipFile.once("close", function() {
          Promise.all(pendingWrites).then(resolve).catch(reject);
        });
      });
    });
  }

  function getArch() {
    if (os.arch() === "arm64") {
      return "aarch64";
    } else if (os.arch() === "x64") {
      return "x86_64";
    } else {
      throw new Error("Unsupported architecture " + os.arch() + ". Only x64 and aarch64 binaries are available.");
    }
  }

  function getLinuxFamily() {
    return getIsMusl() ? "musl" : "gnu";

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
        const report = process.report.getReport();
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
}

module.exports = {
  runInstall() {
    return install().catch(err => {
      if (err !== undefined && typeof err.message === "string") {
        console.error(err.message);
      } else {
        console.error(err);
      }
      process.exit(1);
    });
  },
};

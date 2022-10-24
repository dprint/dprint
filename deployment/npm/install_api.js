// @ts-check
"use strict";

const https = require("https");
const fs = require("fs");
const crypto = require("crypto");
const os = require("os");
const path = require("path");
const yauzl = require("yauzl");

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
  const url = "https://github.com/dprint/dprint/releases/download/"
    + info.version
    + "/dprint-" + target + ".zip";

  // remove the old zip file if it exists
  try {
    fs.unlinkSync(zipFilePath);
  } catch (err) {
    // ignore
  }

  // now try to download it
  return downloadZipFile(url).then(() => {
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
      if (os.arch() === "arm64") {
        return "aarch64-apple-darwin";
      } else if (os.arch() === "x64") {
        return "x86_64-apple-darwin";
      } else {
        throw new Error("Unsupported architecture " + os.arch() + ". Only x64 and M1 binaries are available.");
      }
    } else {
      if (os.arch() === "arm64") {
        return "aarch64-unknown-linux-gnu";
      } else if (os.arch() === "x64") {
        return "x86_64-unknown-linux-gnu";
      } else {
        throw new Error("Unsupported architecture " + os.arch() + ". Only x64 and aarch64 binaries are available.");
      }
    }
  }

  function downloadZipFile(url) {
    return new Promise((resolve, reject) => {
      https.get(url, function(response) {
        if (response.statusCode >= 200 && response.statusCode <= 299) {
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
              resolve();
            }
          });
        });
      });
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
      switch (os.platform()) {
        case "win32":
          return info.checksums["windows-x86_64"];
        case "darwin":
          if (os.arch() === "arm64") {
            return info.checksums["darwin-aarch64"];
          } else {
            return info.checksums["darwin-x86_64"];
          }
        default:
          if (os.arch() === "arm64") {
            return info.checksums["linux-aarch64"];
          } else {
            return info.checksums["linux-x86_64"];
          }
      }
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
                    resolve();
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

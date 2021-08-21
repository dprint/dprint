// @ts-check
"use strict";

const fs = require("fs");
const crypto = require("crypto");
const os = require("os");
const path = require("path");

const info = JSON.parse(fs.readFileSync(path.join(__dirname, "info.json"), "utf8"));
const fileData = fs.readFileSync("dprint.zip");
const actualZipChecksum = crypto.createHash("sha256").update(fileData).digest("hex").toLowerCase();
const expectedZipChecksum = getExpectedZipChecksum().toLowerCase();

if (actualZipChecksum !== expectedZipChecksum) {
  console.error(
    "Downloaded dprint zip checksum did not match the expected checksum (Actual: "
      + actualZipChecksum
      + ", Expected: "
      + expectedZipChecksum
      + ").",
  );
  process.exit(1);
}

function getExpectedZipChecksum() {
  switch (os.platform()) {
    case "win32":
      return info.checksums.windows;
    case "darwin":
      return info.checksums.mac;
    default:
      return info.checksums.linux;
  }
}

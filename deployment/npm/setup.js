// @ts-check
const path = require("path");
const fs = require("fs");
const version = process.argv.slice(2)[0];

// Update the version in package.json
const packageJsonFilePath = path.join(__dirname, "package.json");
const packageJson = JSON.parse(fs.readFileSync(packageJsonFilePath, "utf8"));
packageJson.version = version;
fs.writeFileSync(packageJsonFilePath, JSON.stringify(packageJson, null, 2));

// Update the checksums in info.json
const infoJsonFilePath = path.join(__dirname, "info.json");
const checksums = getChecksums();
const infoJson = {
  version,
  checksums: {
    "windows-x86_64": checksums.get("dprint-x86_64-pc-windows-msvc.zip"),
    "darwin-x86_64": checksums.get("dprint-x86_64-apple-darwin.zip"),
    "darwin-aarch64": checksums.get("dprint-aarch64-apple-darwin.zip"),
    "linux-x86_64": checksums.get("dprint-x86_64-unknown-linux-gnu.zip"),
    "linux-aarch64": checksums.get("dprint-aarch64-unknown-linux-gnu.zip"),
  },
};
fs.writeFileSync(infoJsonFilePath, JSON.stringify(infoJson, null, 2));

function getChecksums() {
  const checksumLines = fs.readFileSync(path.join(__dirname, "SHASUMS256.txt"), "utf8").split(/\r?\n/);
  const checksums = {};
  for (const line of checksumLines) {
    const [checksum, fileName] = line.split(" ");
    checksums[fileName] = checksum;
  }
  return {
    get(fileName) {
      const value = checksums[fileName];
      if (value == null) {
        throw new Error("Could not find " + fileName);
      }
      return value;
    },
  };
}

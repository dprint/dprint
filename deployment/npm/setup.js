// @ts-check
const path = require("path");
const fs = require("fs");
const version = process.argv.slice(2)[0];

// Update the version in package.json
const packageJsonFilePath = path.join(__dirname, "package.json");
const packageJson = JSON.parse(fs.readFileSync(packageJsonFilePath, "utf8"));
packageJson.version = version;
fs.writeFileSync(packageJsonFilePath, JSON.stringify(packageJson, null, 2));

// Update the checksums in info.json (only supports x64 at the moment)
const infoJsonFilePath = path.join(__dirname, "info.json");
const infoJson = JSON.parse(fs.readFileSync(infoJsonFilePath, "utf8"));
const checksums = getChecksums();
infoJson.version = version;
infoJson.checksums.windows = checksums.get("dprint-x86_64-pc-windows-msvc.zip");
infoJson.checksums.mac = checksums.get("dprint-x86_64-apple-darwin.zip");
infoJson.checksums.linux = checksums.get("dprint-x86_64-unknown-linux-gnu.zip");
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
                throw new Error("Could not find " + value);
            }
            return value;
        },
    };
}

export function getPackageVersion() {
    const pjson = require("../../package.json");
    return pjson.version;
}

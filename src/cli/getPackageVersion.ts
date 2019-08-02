export function getPackageVersion() {
    return getJson().version as string;

    function getJson() {
        if (__dirname.endsWith("dist"))
            return require("../package.json");

        return require("../../package.json");
    }
}

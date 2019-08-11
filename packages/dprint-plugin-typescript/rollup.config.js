import typescript from "rollup-plugin-typescript2";
import replace from "rollup-plugin-replace";
import * as fs from "fs";

export default {
    input: "./src/index.ts",
    output: {
        file: "./dist/dprint-plugin-typescript.js",
        format: "cjs"
    },
    plugins: [
        typescript({
            typescript: require("ttypescript"),
            tsconfig: "tsconfig.rollup.json"
        }),
        replace({
            PACKAGE_VERSION: getVersion()
        })
    ]
}

function getVersion() {
    const version = JSON.parse(fs.readFileSync("package.json", { encoding: "utf8" })).version;
    if (!/^[0-9]+\.[0-9]+\.[0-9]+$/.test(version))
        throw new Error("Could not find version. Found: " + version);
    return version;
}

import typescript from "rollup-plugin-typescript2";

export default {
    input: "./src/index.ts",
    output: {
        file: "./dist/dprint-development.js",
        format: "cjs",
    },
    plugins: [
        typescript({
            typescript: require("ttypescript"),
            tsconfig: "tsconfig.rollup.json",
        }),
    ],
};

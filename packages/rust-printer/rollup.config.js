import typescript from "rollup-plugin-typescript2";

export default {
    input: "./src-ts/index.ts",
    external: [
        "./pkg/dprint_rust_printer"
    ],
    output: {
        file: "./dist/dprint-rust-printer.js",
        format: "cjs"
    },
    plugins: [
        typescript({
            typescript: require("ttypescript"),
            tsconfig: "tsconfig.rollup.json"
        })
    ]
};

// @ts-check
import { TypeScriptPlugin } from "./packages/dprint-plugin-typescript";
import { JsoncPlugin } from "./packages/dprint-plugin-jsonc";

/** @type { import("./packages/dprint").Configuration } */
const config = {
    projectType: "openSource",
    lineWidth: 160,
    plugins: [
        new TypeScriptPlugin({
            useBraces: "preferNone",
            "tryStatement.nextControlFlowPosition": "sameLine"
        }),
        new JsoncPlugin({
            indentWidth: 2
        })
    ]
};

export default config;

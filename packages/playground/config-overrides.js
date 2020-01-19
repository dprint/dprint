const path = require('path');
const WasmPackPlugin = require("@wasm-tool/wasm-pack-plugin");

module.exports = function override(config, env) {
    config.plugins.push(new WasmPackPlugin({
        crateDirectory: path.resolve(__dirname, "./wasm"),
        outDir: path.resolve(__dirname, "./src/pkg")
    }));

    // Make file-loader ignore WASM files (https://stackoverflow.com/a/59720645/188246)
    const wasmExtensionRegExp = /\.wasm$/;
    config.module.rules.forEach((rule) => {
        (rule.oneOf || []).forEach((oneOf) => {
            if (oneOf.loader && oneOf.loader.indexOf('file-loader') >= 0) {
                oneOf.exclude.push(wasmExtensionRegExp);
            }
        });
    });

    return config;
};
